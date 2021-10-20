#[macro_use]
extern crate trackable;

use fibers::sync::mpsc::{self, Receiver};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::net::SocketAddr;
use structopt::StructOpt;
use trackable::error::MainError;

#[derive(Debug, Clone, StructOpt)]
struct Opt {
    addr: std::net::IpAddr,

    #[structopt(long, default_value = "8334")]
    port: u16,
}

fn main() -> Result<(), MainError> {
    let opt = Opt::from_args();
    let logger = track!(TerminalLoggerBuilder::new()
        .destination(Destination::Stderr)
        .level("debug".parse().unwrap())
        .build())?; // info or debug

    /* server should use local ip or 0.0.0.0 client should connect through global ip address */
    println!("*{}", local_ipaddress::get().unwrap());
    let addr: SocketAddr = (opt.addr, opt.port).into();

    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem? I mean everyone has different ips...

    let (message_tx, message_rx) = mpsc::channel();
    let node = TestNode {
        node: NodeBuilder::new()
            .logger(logger.clone())
            .finish(service.handle()),
        receiver: message_rx,
        opt,
    };

    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);

    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        // click enter to send a message or attempt to contact someone
        for line in stdin.lock().lines() {
            let line = if let Ok(line) = line {
                line.as_bytes().to_vec()
            } else {
                break;
            };
            if message_tx.send(line).is_err() {
                println!("message send was error!");
                break;
            }
        }
    });

    track_any_err!(executor.run()).unwrap();

    Ok(())
}

/// the node used to run all the networking
struct TestNode {
    node: Node<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
    opt: Opt,
}
impl Future for TestNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(msg)) = track_try_unwrap!(self.node.poll()) {
                println!(
                    "# MESSAGE: {:?}",
                    String::from_utf8_lossy(&msg.message.payload[..])
                );

                println!(
                    "all plumtree peers: {:?}",
                    self.node.plumtree_node().all_push_peers()
                );
                self.node.handle_gossip_now(msg, true);
                did_something = true;
            }
            while let Async::Ready(Some(msg)) = self.receiver.poll().expect("Never fails") {
                // this if statement is how the entrypoint runs. Type in *[ IPv4 ] here (in example is the following: "*192.168.0.101")
                if msg.get(0) == Some(&42)
                /* * */
                {
                    // *192.168.0.101
                    let addr: SocketAddr = track_any_err!(format!(
                        "{}:{}",
                        String::from_utf8_lossy(&msg[1..]),
                        self.opt.port
                    )
                    .parse())
                    .unwrap();
                    let nodeid = NodeId::new(addr, LocalNodeId::new(0));
                    self.node
                        .dm("hello!".as_bytes().to_vec(), &vec![nodeid], true);
                } else {
                    self.node.broadcast(msg);
                }
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
