#[macro_use]
extern crate clap;
#[macro_use]
extern crate trackable;

use clap::Arg;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator, UnixtimeLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::net::SocketAddr;
use trackable::error::MainError;


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::VecDeque;
use std::time::Instant;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::validation::*;




use serde::Serialize;
pub fn hash_to_scalar<T: Serialize> (message: &T) -> Scalar {
    let message = bincode::serialize(message).unwrap();
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
} /* this is for testing purposes. it is used to check if 2 long messages are identicle */



fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("PORT").index(1).required(true))
        .arg(
            Arg::with_name("CONTACT_SERVER").index(2)
                .long("contact-server")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("LOG_LEVEL")
                .long("log-level")
                .takes_value(true)
                .default_value("info")
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
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.101:{}", port).parse())?;

    let addr: SocketAddr = track_any_err!(format!("128.61.4.96:{}", port).parse())?; // gatech
    

    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    



    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem? I mean everyone has different ips...
        // .finish(executor.handle(), UnixtimeLocalNodeIdGenerator::new());
        
    let mut node = NodeBuilder::new().logger(logger).finish(service.handle());
    println!("{:?}",node.id());
    if let Some(contact) = matches.value_of("CONTACT_SERVER") {
        println!("contact: {:?}",contact);
        let contact: SocketAddr = track_any_err!(contact.parse())?;
        node.join(NodeId::new(contact, LocalNodeId::new(0)));
    }

    let l = Account::new(&format!("{}",0)).stake_acc();
    let leader = l.receive_ot(&l.derive_stk_ot(&Scalar::one())).unwrap(); //make a new account
    let lkey = leader.sk.unwrap();
    let keylocation = 0;
    let staker0 = leader.pk.compress();
    let node = LeaderNode {
        inner: node,
        key: lkey,
        keylocation: keylocation,
        stkinfo: vec![(staker0,1u64)],
        txses: vec![],
        lastblock: NextBlock::default(),
        sigs: vec![],
        queue: (0..max_shards).map(|_|[0usize;NUMBER_OF_VALIDATORS as usize].into_par_iter().collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|vec![0usize;NUMBER_OF_VALIDATORS as usize]).collect::<Vec<_>>(),
        lastname: vec![],
        bnum: 1u64,
        timekeeper: Instant::now(),
    };
    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    track_any_err!(executor.run())?;
    Ok(())
}

struct LeaderNode {
    inner: Node<Vec<u8>>,
    key: Scalar,
    keylocation: u64,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    txses: Vec<Vec<u8>>,
    lastblock: NextBlock,
    sigs: Vec<NextBlock>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    timekeeper: Instant,
}
impl Future for LeaderNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(m)) = track_try_unwrap!(self.inner.poll()) {
                let mut m = m.payload().to_vec();
                if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a leader
                    if mtype == 2 {print!("#{:?}", mtype);}
                    else {println!("# MESSAGE TYPE: {:?}", mtype);}
                    if mtype == 0 {
                        self.txses.push(m[..std::cmp::min(m.len(),10_000)].to_vec());
                    }
                    else if mtype == 2 {
                        self.sigs.push(bincode::deserialize(&m).unwrap());
                    }
                }
                // println!("pt id: {:?}",self.inner.plumtree_node().id());
                // println!("pt epp: {:?}",self.inner.plumtree_node().eager_push_peers());
                // println!("pt lpp: {:?}",self.inner.plumtree_node().lazy_push_peers());
                // println!("hv id: {:?}",self.inner.hyparview_node().id());
                // println!("hv av: {:?}",self.inner.hyparview_node().active_view());
                // println!("hv pv: {:?}",self.inner.hyparview_node().passive_view());
                did_something = true;
            }
            if (self.sigs.len() >= (2*(NUMBER_OF_VALIDATORS/3)).into()) | ( (self.sigs.len() >= (NUMBER_OF_VALIDATORS/3).into()) & (self.timekeeper.elapsed().as_secs() > 30) ) {
                let shard = 0;
                // println!("time:::{:?}",self.timekeeper.elapsed().as_secs()); // that's not it
                let lastblock = NextBlock::finish(&self.key, &self.keylocation, &self.sigs.drain(..).collect::<Vec<_>>(), &self.comittee[shard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(shard as u16), &self.bnum, &self.lastname, &self.stkinfo);

                if lastblock.validators.len() != 0 {
                    self.lastblock = lastblock;
                    let mut m = bincode::serialize(&self.lastblock).unwrap();
    
                    self.sigs = vec![];

                    let mut hasher = Sha3_512::new();
                    hasher.update(&m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                    // println!("{:?}",hash_to_scalar(&self.lastblock));


                    m.push(3u8);
                    self.inner.broadcast(m);
                    self.lastblock.scan_as_noone(&mut self.stkinfo,&self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>(), &mut self.queue, &mut self.exitqueue, &mut self.comittee);
                    
                    
                    println!("{:?}",self.stkinfo);


                    for i in 0..self.comittee.len() {
                        select_stakers(&self.lastname, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                    println!("made a block with {} transactions!",self.lastblock.txs.len());
                    did_something = true;
                }
                else {
                    println!("failed to make a block :(");
                    did_something = false;
                }

                self.timekeeper = Instant::now();
            }
            if /*(self.txses.len() > 0) |*/ (self.timekeeper.elapsed().as_secs() > 10) /*(self.txses.len() >= 512) | (self.timekeeper.elapsed().as_secs() > 60/self.bnum + 1)*/ { // make this floating point too for time
                self.sigs = vec![];
                let mut m = bincode::serialize(&self.txses).unwrap();
                m.push(1u8);
                self.inner.broadcast(m);
                self.txses = vec![];
                self.timekeeper = Instant::now();
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
