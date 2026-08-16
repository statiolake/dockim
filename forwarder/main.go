// dockim-forwarder is the data-plane half of Dockim's Docker exec tunnel.
//
// It intentionally writes only application bytes to stdout. Docker exec's stdin and stdout are
// connected to a host-side TCP connection by Dockim, while this process connects to the service
// inside the container's own network namespace.
package main

import (
	"errors"
	"fmt"
	"io"
	"net"
	"os"
)

func main() {
	if len(os.Args) != 2 {
		_, _ = fmt.Fprintln(os.Stderr, "usage: dockim-forwarder HOST:PORT")
		os.Exit(2)
	}

	connection, err := net.Dial("tcp", os.Args[1])
	if err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	inputDone := make(chan error, 1)
	outputDone := make(chan error, 1)

	go func() {
		_, copyErr := io.Copy(connection, os.Stdin)
		if tcp, ok := connection.(*net.TCPConn); ok {
			_ = tcp.CloseWrite()
		}
		inputDone <- copyErr
	}()

	go func() {
		_, copyErr := io.Copy(os.Stdout, connection)
		outputDone <- copyErr
	}()

	inputErr := <-inputDone
	outputErr := <-outputDone
	_ = connection.Close()

	if meaningfulError(inputErr) {
		_, _ = fmt.Fprintln(os.Stderr, inputErr)
		os.Exit(1)
	}
	if meaningfulError(outputErr) {
		_, _ = fmt.Fprintln(os.Stderr, outputErr)
		os.Exit(1)
	}
}

func meaningfulError(err error) bool {
	return err != nil && !errors.Is(err, net.ErrClosed) && !errors.Is(err, io.EOF)
}
