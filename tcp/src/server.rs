// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

use std;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use tokio_core::io::Io;
use tokio_service::Service as TokioService;

use jsonrpc::{MetaIoHandler, Metadata};
use jsonrpc::futures::{future, Future, Stream, Sink};
use jsonrpc::futures::sync::mpsc;
use service::Service;
use line_codec::LineCodec;
use meta::{MetaExtractor, RequestContext, NoopExtractor};
use dispatch::{Dispatcher, SenderChannels, PeerMessageQueue};

pub struct Server<M: Metadata = ()> {
    listen_addr: SocketAddr,
    handler: Arc<MetaIoHandler<M>>,
    meta_extractor: Arc<MetaExtractor<M>>,
    channels: Arc<SenderChannels>,
}

impl<M: Metadata> Server<M> {
    pub fn new(addr: SocketAddr, handler: Arc<MetaIoHandler<M>>) -> Self {
        Server {
            listen_addr: addr,
            handler: handler,
            meta_extractor: Arc::new(NoopExtractor),
            channels: Default::default(),
        }
    }

    pub fn extractor(mut self, meta_extractor: Arc<MetaExtractor<M>>) -> Self {
        self.meta_extractor = meta_extractor;
        self
    }

    pub fn run(&self) -> std::io::Result<()> {
        let mut core = Core::new()?;
        let handle = core.handle();
        let meta_extractor = self.meta_extractor.clone();

        let listener = TcpListener::bind(&self.listen_addr, &handle)?;

        let connections = listener.incoming();
        let server = connections.for_each(move |(socket, peer_addr)| {
            trace!(target: "tcp", "Accepted incoming connection from {}", &peer_addr);

            let context = RequestContext { peer_addr: peer_addr };
            let meta = meta_extractor.extract(&context);

            let (writer, reader) = socket.framed(LineCodec).split();
            let service = self.spawn_service(peer_addr, meta);

            let responses = reader.and_then(
                move |req| service.call(req).then(|response|
                    match response {
                        Err(e) => {
                            warn!(target: "tcp", "Error while processing request: {:?}", e);
                            future::ok(String::new())
                        },
                        Ok(None) => {
                            trace!(target: "tcp", "JSON RPC request produced no response");
                            future::ok(String::new())
                        },
                        Ok(Some(response_data)) => {
                            trace!(target: "tcp", "Sent response: {}", &response_data);
                            future::ok(response_data)
                        }
                    }));

            let peer_message_queue = {
                let (sender, receiver) = mpsc::channel(65536);
                let mut channels = self.channels.lock().unwrap();
                channels.insert(peer_addr.clone(), sender.clone());

                PeerMessageQueue::new(
                    responses,
                    receiver,
                    peer_addr.clone(),
                )
            };

            let shared_channels = self.channels.clone();
            let server = writer.send_all(peer_message_queue).then(
                move |_| {
                    trace!(target: "tcp", "Peer {}: service finished", peer_addr);
                    let mut channels = shared_channels.lock().unwrap();
                    channels.remove(&peer_addr);
                    Ok(())
                }
            );
            handle.spawn(server);

            Ok(())
        });
        core.run(server)
    }

    pub fn dispatcher(&self) -> Dispatcher {
        Dispatcher::new(self.channels.clone())
    }

    fn spawn_service(&self, peer_addr: SocketAddr, meta: M) -> Service<M> {
        Service::new(peer_addr.clone(), self.handler.clone(), meta)
    }
}
