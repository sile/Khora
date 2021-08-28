#[macro_use]
extern crate clap;
#[macro_use]
extern crate trackable;

use clap::Arg;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator, UnixtimeLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::net::SocketAddr;
use trackable::error::MainError;
 // cargo run --bin chat --release 6060 
fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("PORT").index(1).required(true))
        .arg(
            Arg::with_name("CONTACT_SERVER_0").index(2)
                .long("contact-server-0")
                .takes_value(true),
        )        .arg(
            Arg::with_name("CONTACT_SERVER_1").index(3)
                .long("contact-server-1")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("LOG_LEVEL")
                .long("log-level")
                .takes_value(true)
                .default_value("debug")
                .possible_values(&["debug", "info"]),
        )
        .get_matches();
    let log_level = track_any_err!(matches.value_of("LOG_LEVEL").unwrap().parse())?;
    let logger = track!(TerminalLoggerBuilder::new()
        .destination(Destination::Stderr)
        .level(log_level)
        .build())?;
    let port = matches.value_of("PORT").unwrap();
    println!("port: {:?}",port);
    // let addr: SocketAddr = track_any_err!(format!("127.0.0.1:{}", port).parse())?; // ip r | grep default <--- router ip, just go to settings
    // let addr: SocketAddr = track_any_err!(format!("172.20.10.14:{}", port).parse())?;
    // let addr: SocketAddr = track_any_err!(format!("172.16.0.8:{}", port).parse())?;
    let addr: SocketAddr = track_any_err!(format!("192.168.00.101:{}", port).parse())?; // waf stampede
    // let addr: SocketAddr = track_any_err!(format!("172.20.10.3:{}", port).parse())?; // my iphone
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.1:{}", port).parse())?;
    
    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem?
        // .finish(executor.handle(), UnixtimeLocalNodeIdGenerator::new());
        
    let mut node = NodeBuilder::new().logger(logger).finish(service.handle());
    println!("{:?}",node.id());
    if let Some(contact) = matches.value_of("CONTACT_SERVER_0") {
        println!("contact: {:?}",contact);
        let contact: SocketAddr = track_any_err!(contact.parse())?;
        node.join(NodeId::new(contact, LocalNodeId::new(0)));
    }
    if let Some(contact) = matches.value_of("CONTACT_SERVER_1") {
        println!("{:?}",contact);
        let contact: SocketAddr = track_any_err!(contact.parse())?;
        node.join(NodeId::new(contact, LocalNodeId::new(0)));
    }
            
    let (message_tx, message_rx) = mpsc::channel();
    let node = ChatNode {
        inner: node,
        message_rx,
    };
    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);

    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            // println!("line sent: {:?}",line);
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

    track_any_err!(executor.run())?;
    Ok(())
}

struct ChatNode {
    inner: Node<Vec<u8>>,
    message_rx: mpsc::Receiver<Vec<u8>>,
}
impl Future for ChatNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(m)) = track_try_unwrap!(self.inner.poll()) {
                println!("# MESSAGE (content): {:?}", String::from_utf8_lossy(&m.payload()[..]));
                println!("# MESSAGE: {:?}", m.payload().len());

                println!("pt id: {:?}",self.inner.plumtree_node().id());
                println!("pt epp: {:?}",self.inner.plumtree_node().eager_push_peers());
                println!("pt lpp: {:?}",self.inner.plumtree_node().lazy_push_peers());
                println!("hv id: {:?}",self.inner.hyparview_node().id());
                println!("hv av: {:?}",self.inner.hyparview_node().active_view());
                println!("hv pv: {:?}",self.inner.hyparview_node().passive_view());
                did_something = true;
            }
            while let Async::Ready(Some(m)) = self.message_rx.poll().expect("Never fails") {
                // // println!("# MESSAGE (sent): {:?}", m);
                // // println!("pt id: {:?}",self.inner.plumtree_node().id());
                // // println!("pt epp: {:?}",self.inner.plumtree_node().eager_push_peers());
                // // println!("pt lpp: {:?}",self.inner.plumtree_node().lazy_push_peers());
                // // // println!("pt m: {:?}",self.inner.plumtree_node().messages());
                // // // println!("pt wm: {:?}",self.inner.plumtree_node().waiting_messages());
                // // println!("pt c: {:?}",self.inner.plumtree_node().clock());
                // // println!("hv id: {:?}",self.inner.hyparview_node().id());
                // // println!("hv av: {:?}",self.inner.hyparview_node().active_view());
                // // println!("hv pv: {:?}",self.inner.hyparview_node().passive_view());
                // let x = self.inner.hyparview_node().active_view().into_iter().map(|x|x.address()).collect::<Vec<SocketAddr>>();
                // let y = self.inner.hyparview_node().passive_view().into_iter().map(|x|x.address()).collect::<Vec<SocketAddr>>();
                // // let x = bincode::serialize(self.inner.hyparview_node().id()).unwrap();
                // let x = bincode::serialize(&x).unwrap();
                // let y = bincode::serialize(&y).unwrap();
                // println!("active: {:?}\npassive: {:?}",x,y);
                // // let m = vec![0;100_000_000];
                // // println!("service ln: {:?}",self.inner.service.local_nodes());
                if m == vec![109] /* m */ {
                    self.inner.mute_all();
                    println!("MUTE");
                } else if m == vec![117] /* u */ {
                    self.inner.unmute_all();
                    println!("UNMUTE");
                } else if m == vec![105] /* i */ {
                    println!("------------------------------------------------------------------------------------------------------------------------------------------------");
                    println!("METRICS: {:#?}",self.inner.metrics());
                    println!("------------------------------------------------------------------------------------------------------------------------------------------------");
                    println!("SURVICE METRICS: {:#?}",self.inner.service.metrics());
                    println!("------------------------------------------------------------------------------------------------------------------------------------------------");
                    println!("RPC SURVICE METRICS: {:#?}",self.inner.service.rpc_service().metrics());
                    println!("------------------------------------------------------------------------------------------------------------------------------------------------");
                    println!("RPC SURVICE CHANNELS: {:#?}",self.inner.service.rpc_service().channels().load());
                    println!("------------------------------------------------------------------------------------------------------------------------------------------------");
                } else if m.get(0) == Some(&97) /* a */ { // a192.168.000.101:09876hi!
                    let you: SocketAddr = track_any_err!(String::from_utf8_lossy(&m[1..22]).parse()).unwrap();
                    let ip = NodeId::new(you, LocalNodeId::new(0));
                    self.inner.dm(m[22..].to_vec(),&vec![ip],false);
                } else {
                    self.inner.broadcast(m);
                }
                did_something = true;

            }
        }
        Ok(Async::NotReady)
    }
}
