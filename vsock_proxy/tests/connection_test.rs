// Copyright 2019 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
#![deny(warnings)]

use nix::sys::socket::SockAddr;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::{IpAddr, Ipv4Addr};
use std::str;
use std::sync::mpsc;
use std::{process, thread};
use tempfile::NamedTempFile;
use vsock::VsockStream;

use vsock_proxy::starter::{Proxy, ProxyError};

fn vsock_connect(port: u32) -> Result<VsockStream, ProxyError> {
    let sockaddr = SockAddr::new_vsock(vsock_proxy::starter::VSOCK_PROXY_CID, port);
    VsockStream::connect(&sockaddr).map_err(|_e| ProxyError::ConnectError)
}

/// Test connection with both client and server sending each other messages
#[test]
fn test_tcp_connection() {
    // Proxy will translate from port 8000 vsock to localhost port 9000 TCP
    let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(
        b"whitelist:\n\
            - {address: 127.0.0.1, port: 9000}",
    )
    .unwrap();
    let proxy = Proxy::new(
        vsock_proxy::starter::VSOCK_PROXY_PORT,
        addr,
        9000,
        2,
        file.path().to_str(),
        false,
        false,
    );

    let (tx, rx) = mpsc::channel();

    // Create a listening TCP server on port 9000
    let server_handle = thread::spawn(move || {
        let server = TcpListener::bind("127.0.0.1:9000").expect("server bind");
        tx.send(true).expect("server send event");
        let (mut stream, _) = server.accept().expect("server accept");

        // Read request
        let mut buf = [0; 13];
        stream.read_exact(&mut buf).expect("server read");
        let msg = str::from_utf8(&buf).expect("from_utf8");
        assert_eq!(msg, "client2server");

        // Write response
        stream.write_all(b"server2client").expect("server write");
    });

    let _ret = rx.recv().expect("main recv event");
    let (tx, rx) = mpsc::channel();

    // Start proxy in a different thread
    let ret = proxy.sock_listen();
    let listener = ret.expect("proxy listen");
    let proxy_handle = thread::spawn(move || {
        tx.send(true).expect("proxy send event");
        let _ret = proxy.sock_accept(&listener).expect("proxy accept");
    });

    let _ret = rx.recv().expect("main recv event");

    // Start client that connects to proxy on port 8000 vsock
    let client_handle = thread::spawn(move || {
        let ret = vsock_connect(vsock_proxy::starter::VSOCK_PROXY_PORT);
        if ret.is_err() {
            eprintln!("{:?}", ret.err());
            process::exit(1);
        }
        let mut stream = ret.unwrap();

        // Write request
        stream.write_all(b"client2server").expect("client write");

        // Read response
        let mut buf = [0; 13];

        stream.read_exact(&mut buf).expect("client read");
        let msg = str::from_utf8(&buf).expect("from_utf8");
        assert_eq!(msg, "server2client");
    });

    server_handle.join().expect("Server panicked");
    proxy_handle.join().expect("Proxy panicked");
    client_handle.join().expect("Client panicked");
}