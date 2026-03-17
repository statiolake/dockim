use std::{
    collections::HashMap,
    io::{IsTerminal, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

use crossterm::{cursor, execute, terminal};
use unicode_width::UnicodeWidthStr;

type NodeId = u64;

struct ConsoleNode {
    parent: Option<NodeId>,
    committed: Vec<String>,
    live_lines: Vec<String>,
    children: Vec<NodeId>,
}

struct RootState {
    nodes: HashMap<NodeId, ConsoleNode>,
    root_id: NodeId,
    next_id: u64,
    rendered_live_lines: usize,
    root_committed_flushed: usize,
    is_tty: bool,
    last_render: Instant,
}

/// Console manages a tree of output nodes with committed (permanent) and live (ephemeral) zones.
///
/// Root committed lines are flushed to stderr once and never re-drawn.
/// Children of root form the "live zone" which is cleared and re-drawn on each update.
/// When a child Console is dropped, its content moves to the parent's committed zone.
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
                committed: Vec::new(),
                live_lines: Vec::new(),
                children: Vec::new(),
            },
        );

        Console {
            state: Arc::new(Mutex::new(RootState {
                nodes,
                root_id,
                next_id: 1,
                rendered_live_lines: 0,
                root_committed_flushed: 0,
                is_tty: std::io::stderr().is_terminal(),
                last_render: Instant::now(),
            })),
            node_id: root_id,
            owns_node: false,
        }
    }

    /// Add text to the committed zone. Each line in text becomes a committed line.
    /// Triggers a re-render (or immediate print in non-TTY mode).
    pub fn write_line(&self, text: &str) {
        if text.is_empty() {
            return;
        }

        let mut state = self.state.lock().unwrap();
        let Some(node) = state.nodes.get_mut(&self.node_id) else {
            return; // node already committed
        };

        let lines: Vec<&str> = text.lines().collect();
        for line in &lines {
            node.committed.push((*line).to_string());
        }

        if state.is_tty {
            Self::render_tty(&mut state);
        } else {
            // Non-TTY: print immediately
            let mut stderr = std::io::stderr();
            for line in &lines {
                let _ = writeln!(stderr, "{line}");
            }
        }
    }

    /// Create a child Console in the live zone. The child is committed to the parent on drop.
    pub fn child(&self) -> Console {
        let mut state = self.state.lock().unwrap();
        let child_id = state.next_id;
        state.next_id += 1;

        state.nodes.insert(
            child_id,
            ConsoleNode {
                parent: Some(self.node_id),
                committed: Vec::new(),
                live_lines: Vec::new(),
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

    /// Set the live lines for this node. Replaces any previous live lines.
    /// Used for tail output that is overwritten on each update.
    /// No-op in non-TTY mode.
    pub fn set_live_lines(&self, lines: Vec<String>) {
        let mut state = self.state.lock().unwrap();
        if !state.is_tty {
            return;
        }
        let Some(node) = state.nodes.get_mut(&self.node_id) else {
            return;
        };
        node.live_lines = lines;

        // Debounce: skip render if last render was < 16ms ago
        let now = Instant::now();
        if now.duration_since(state.last_render).as_millis() >= 16 {
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
        if state.rendered_live_lines == 0 {
            return;
        }
        let mut stderr = std::io::stderr();
        let _ = execute!(
            stderr,
            cursor::MoveUp(state.rendered_live_lines as u16),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        );
        state.rendered_live_lines = 0;
    }

    fn render_live_zone(state: &mut RootState) {
        let mut lines = Vec::new();
        let root_id = state.root_id;

        // Collect from root's children (not root's own committed—already flushed)
        let child_ids: Vec<NodeId> = state.nodes[&root_id].children.clone();
        for child_id in child_ids {
            Self::collect_node_lines(state, child_id, &mut lines);
        }
        // Root's own live lines (rarely used, but supported)
        lines.extend(state.nodes[&root_id].live_lines.iter().cloned());

        if lines.is_empty() {
            return;
        }

        let width = terminal::size().map(|(w, _)| w as usize).unwrap_or(120);
        let mut stderr = std::io::stderr();
        let mut physical_lines = 0;

        for line in &lines {
            let display_len = UnicodeWidthStr::width(strip_ansi_codes(line).as_str());
            let phys = if width > 0 && display_len > width {
                (display_len + width - 1) / width
            } else {
                1
            };
            let _ = writeln!(stderr, "{line}");
            physical_lines += phys;
        }

        state.rendered_live_lines = physical_lines;
    }

    fn collect_node_lines(state: &RootState, node_id: NodeId, out: &mut Vec<String>) {
        let Some(node) = state.nodes.get(&node_id) else {
            return;
        };
        out.extend(node.committed.iter().cloned());
        let child_ids: Vec<NodeId> = node.children.clone();
        for child_id in child_ids {
            Self::collect_node_lines(state, child_id, out);
        }
        out.extend(node.live_lines.iter().cloned());
    }

    // --- Node lifecycle ---

    /// Move all content from a node into its parent's committed zone, then remove the node.
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

        // Remove from parent's children list
        if let Some(parent) = state.nodes.get_mut(&parent_id) {
            parent.children.retain(|&id| id != node_id);
            // Move committed + live lines to parent committed
            parent.committed.extend(node.committed);
            parent.committed.extend(node.live_lines);
        }
    }

    /// Remove a node and all its descendants without moving content.
    /// Used in non-TTY mode where content was already printed.
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
