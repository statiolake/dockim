use std::{
    collections::HashMap,
    io::{IsTerminal, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

static SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// RAII guard that suppresses all Console output while alive.
///
/// Use this when handing the terminal to an interactive child process
/// (shell, exec, neovim, …) so background tasks don't corrupt its display.
pub struct SuppressGuard;

impl SuppressGuard {
    pub fn new() -> Self {
        SUPPRESSED.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for SuppressGuard {
    fn drop(&mut self) {
        SUPPRESSED.store(false, Ordering::Relaxed);
    }
}

fn is_suppressed() -> bool {
    SUPPRESSED.load(Ordering::Relaxed)
}

/// Always returns `Stdio::inherit()`, even while a [`SuppressGuard`] is active.
///
/// Use this for the foreground interactive process itself (bash, shell, neovim, …) that
/// intentionally owns the TTY — suppression must not affect its own I/O.
pub fn force_inherit_stdio() -> std::process::Stdio {
    std::process::Stdio::inherit()
}

/// Run `stty sane` to restore the terminal to a sane state after an interactive process exits.
///
/// Only call this from [`exec::run_interactive`] — tail-based execution never touches terminal
/// modes so it doesn't need a reset.
pub async fn reset_terminal() {
    let _ = tokio::process::Command::new("stty")
        .arg("sane")
        .status()
        .await;
}

use crossterm::{cursor, execute, terminal};
use unicode_width::UnicodeWidthStr;

type NodeId = u64;

const INDENT: &str = "    ";

struct ConsoleNode {
    parent: Option<NodeId>,
    indent: String,
    committed: Vec<String>,
    live: Vec<String>,
    children: Vec<NodeId>,
}

struct RootState {
    nodes: HashMap<NodeId, ConsoleNode>,
    root_id: NodeId,
    next_id: u64,
    /// The logical lines actually written to the terminal in the last live-zone render.
    /// Stored so `clear_live_zone` can re-derive the physical row count using the
    /// *current* terminal width rather than the width at render time — making the
    /// clear correct even after a terminal resize.
    last_rendered_lines: Vec<String>,
    root_committed_flushed: usize,
    is_tty: bool,
    last_render: Instant,
}

/// Console manages a tree of output nodes with committed (permanent) and live (ephemeral) zones.
///
/// - **committed**: finalized lines, immutable from outside. Grows when children commit on drop.
/// - **live**: the node's current visual state, fully replaced on each `set_live()` call.
///
/// Rendering order per node: live, committed, then active children (recursively).
/// On commit (drop): live + committed → parent's committed.
///
/// Console applies tree-depth-based indentation automatically. Callers never manage indent.
pub struct Console {
    state: Arc<Mutex<RootState>>,
    node_id: NodeId,
    owns_node: bool,
}

impl Console {
    /// Create a new root Console that writes to stderr.
    pub fn new() -> Self {
        let root_id = 0;
        let mut nodes = HashMap::new();
        nodes.insert(
            root_id,
            ConsoleNode {
                parent: None,
                indent: String::new(),
                committed: Vec::new(),
                live: Vec::new(),
                children: Vec::new(),
            },
        );

        Console {
            state: Arc::new(Mutex::new(RootState {
                nodes,
                root_id,
                next_id: 1,
                last_rendered_lines: Vec::new(),
                root_committed_flushed: 0,
                is_tty: std::io::stderr().is_terminal(),
                last_render: Instant::now(),
            })),
            node_id: root_id,
            owns_node: false,
        }
    }

    pub fn is_tty(&self) -> bool {
        self.state.lock().unwrap().is_tty
    }

    /// Append text to committed zone. Indent is applied automatically.
    /// In non-TTY mode, also prints immediately to stderr.
    pub fn write_line(&self, text: &str) {
        if text.is_empty() || is_suppressed() {
            return;
        }

        let mut state = self.state.lock().unwrap();
        let Some(node) = state.nodes.get_mut(&self.node_id) else {
            return;
        };

        let indent = node.indent.clone();
        let indented: Vec<String> = text
            .lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{indent}{line}")
                }
            })
            .collect();

        node.committed.extend(indented.iter().cloned());

        if state.is_tty {
            Self::render_tty(&mut state);
        } else {
            let mut stderr = std::io::stderr();
            for line in &indented {
                let _ = writeln!(stderr, "{line}");
            }
        }
    }

    /// Create a child Console node. The child is committed to this node on drop.
    ///
    /// Root children have no indent (they are the top-level output).
    /// Deeper children add one indent level relative to their parent.
    pub fn child(&self) -> Console {
        let mut state = self.state.lock().unwrap();
        let child_id = state.next_id;
        state.next_id += 1;

        let parent_node = &state.nodes[&self.node_id];
        let child_indent = if parent_node.parent.is_none() {
            // Root children: top-level, no indent
            String::new()
        } else {
            format!("{}{INDENT}", parent_node.indent)
        };

        state.nodes.insert(
            child_id,
            ConsoleNode {
                parent: Some(self.node_id),
                indent: child_indent,
                committed: Vec::new(),
                live: Vec::new(),
                children: Vec::new(),
            },
        );

        if let Some(parent) = state.nodes.get_mut(&self.node_id) {
            parent.children.push(child_id);
        }

        Console {
            state: self.state.clone(),
            node_id: child_id,
            owns_node: true,
        }
    }

    /// Fully replace this node's live lines. Indent is applied automatically.
    ///
    /// In non-TTY mode, prints on the first non-empty call only (so the header
    /// appears before any child output).
    pub fn set_live(&self, lines: Vec<String>) {
        if is_suppressed() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let indent = match state.nodes.get(&self.node_id) {
            Some(node) => node.indent.clone(),
            None => return,
        };

        let indented: Vec<String> = lines
            .into_iter()
            .map(|l| {
                if l.is_empty() {
                    l
                } else {
                    format!("{indent}{l}")
                }
            })
            .collect();

        let is_tty = state.is_tty;

        let node = state.nodes.get_mut(&self.node_id).unwrap();
        if !is_tty {
            // Non-TTY: print header on first non-empty set only
            if node.live.is_empty() && !indented.is_empty() {
                let mut stderr = std::io::stderr();
                for line in &indented {
                    let _ = writeln!(stderr, "{line}");
                }
            }
            node.live = indented;
            return;
        }

        node.live = indented;

        if state.last_render.elapsed().as_millis() >= 16 {
            Self::render_tty(&mut state);
        }
    }

    // --- TTY rendering ---

    fn render_tty(state: &mut RootState) {
        Self::clear_live_zone(state);
        Self::flush_root_committed(state);
        Self::render_live_zone(state);
        state.last_render = Instant::now();
    }

    fn flush_root_committed(state: &mut RootState) {
        let root_id = state.root_id;
        let committed_len = state.nodes[&root_id].committed.len();
        if state.root_committed_flushed >= committed_len {
            return;
        }
        let mut stderr = std::io::stderr();
        let root = &state.nodes[&root_id];
        for line in &root.committed[state.root_committed_flushed..] {
            let _ = writeln!(stderr, "{line}");
        }
        state.root_committed_flushed = committed_len;
    }

    fn clear_live_zone(state: &mut RootState) {
        if state.last_rendered_lines.is_empty() {
            return;
        }
        // Re-derive physical line count using the CURRENT terminal width.
        // If the terminal was resized since the last render, this gives the correct
        // number of rows to move up rather than a stale cached value.
        let width = terminal::size().map(|(w, _)| w as usize).unwrap_or(120);
        let physical_lines: usize = state
            .last_rendered_lines
            .iter()
            .map(|l| physical_line_count(strip_ansi_codes(l).as_str(), width))
            .sum();

        state.last_rendered_lines.clear();

        if physical_lines == 0 {
            return;
        }
        let mut stderr = std::io::stderr();
        let _ = execute!(
            stderr,
            cursor::MoveUp(physical_lines as u16),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        );
    }

    fn render_live_zone(state: &mut RootState) {
        let mut lines = Vec::new();
        let root_id = state.root_id;

        // Root's children form the live zone
        let child_ids: Vec<NodeId> = state.nodes[&root_id].children.clone();
        for child_id in child_ids {
            Self::collect_node_lines(state, child_id, &mut lines);
        }

        if lines.is_empty() {
            return;
        }

        let (width, height) = terminal::size().unwrap_or((120, 40));
        let width = width as usize;
        // Reserve 1 row so MoveUp(physical_lines) always stays within the viewport.
        let max_physical = (height as usize).saturating_sub(1);

        // Pre-compute physical line count for each logical line.
        let phys_counts: Vec<usize> = lines
            .iter()
            .map(|l| physical_line_count(strip_ansi_codes(l).as_str(), width))
            .collect();

        // Trim from the top: keep only the tail that fits within max_physical.
        // This ensures MoveUp never tries to go beyond the viewport on the next clear.
        let mut physical_lines = 0usize;
        let mut start_idx = lines.len();
        while start_idx > 0 {
            let phys = phys_counts[start_idx - 1];
            if physical_lines + phys > max_physical {
                break;
            }
            physical_lines += phys;
            start_idx -= 1;
        }

        let rendered = &lines[start_idx..];
        let mut stderr = std::io::stderr();
        for line in rendered {
            let _ = writeln!(stderr, "{line}");
        }

        // Store the actual lines written so clear_live_zone can recompute with
        // the then-current terminal width (handles resize between render and clear).
        state.last_rendered_lines = rendered.to_vec();
    }

    /// Collect all visible lines from a node for live zone rendering.
    /// Order: live (header/tail), committed (completed children), active children.
    fn collect_node_lines(state: &RootState, node_id: NodeId, out: &mut Vec<String>) {
        let Some(node) = state.nodes.get(&node_id) else {
            return;
        };

        // 1. This node's live state (step header, verbose, tail)
        out.extend(node.live.iter().cloned());

        // 2. Committed content (output from completed children)
        out.extend(node.committed.iter().cloned());

        // 3. Active children (recursively)
        let child_ids: Vec<NodeId> = node.children.clone();
        for child_id in child_ids {
            Self::collect_node_lines(state, child_id, out);
        }
    }

    // --- Node lifecycle ---

    /// Commit a node to its parent: live + committed → parent.committed.
    fn commit_node(state: &mut RootState, node_id: NodeId) {
        // Recursively commit any remaining children first
        let children: Vec<NodeId> = state
            .nodes
            .get(&node_id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        for child_id in children {
            Self::commit_node(state, child_id);
        }

        let Some(node) = state.nodes.remove(&node_id) else {
            return;
        };
        let Some(parent_id) = node.parent else {
            return;
        };

        if let Some(parent) = state.nodes.get_mut(&parent_id) {
            parent.children.retain(|&id| id != node_id);
            // Order: live (header) first, then committed (children's content)
            parent.committed.extend(node.live);
            parent.committed.extend(node.committed);
        }
    }

    /// Remove a node tree without committing content. Used in non-TTY mode
    /// where content was already printed immediately.
    fn remove_node_tree(state: &mut RootState, node_id: NodeId) {
        let children: Vec<NodeId> = state
            .nodes
            .get(&node_id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        for child_id in children {
            Self::remove_node_tree(state, child_id);
        }
        if let Some(node) = state.nodes.remove(&node_id) {
            if let Some(parent_id) = node.parent {
                if let Some(parent) = state.nodes.get_mut(&parent_id) {
                    parent.children.retain(|&id| id != node_id);
                }
            }
        }
    }
}

impl Clone for Console {
    fn clone(&self) -> Self {
        Console {
            state: self.state.clone(),
            node_id: self.node_id,
            owns_node: false, // clones never own the node
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        if !self.owns_node {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if !state.nodes.contains_key(&self.node_id) {
            return;
        }

        if state.is_tty {
            Self::clear_live_zone(&mut state);
            Self::commit_node(&mut state, self.node_id);
            Self::flush_root_committed(&mut state);
            Self::render_live_zone(&mut state);
            state.last_render = Instant::now();
        } else {
            Self::remove_node_tree(&mut state, self.node_id);
        }
    }
}

/// How many physical terminal rows a single logical line occupies.
///
/// Accounts for line-wrapping based on terminal `width`. Full-width Unicode characters
/// count as 2 columns; east-asian ambiguous characters count as 1 (simplified).
/// `text` must already have ANSI escape codes stripped.
fn physical_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let cols = UnicodeWidthStr::width(text);
    if cols == 0 {
        1 // blank line still occupies one row
    } else {
        (cols + width - 1) / width
    }
}

/// Strip ANSI escape codes from a string for display-width calculation.
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }
    result
}
