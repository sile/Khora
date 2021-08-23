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
use std::convert::TryInto;
use std::net::SocketAddr;
use trackable::error::MainError;


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::validation::*;
use kora::validation::BLOCK_KEYWORD;
// use bimap::BiHashMap;



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

    let addr: SocketAddr = track_any_err!(format!("128.61.8.55:{}", port).parse())?; // gatech
    

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
        // iptable: BiHashMap::new(),
        txses: vec![],
        lastblock: NextBlock::default(),
        sigs: vec![],
        queue: (0..max_shards).map(|_|[0usize;NUMBER_OF_VALIDATORS as usize].into_par_iter().collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|vec![0usize;NUMBER_OF_VALIDATORS as usize]).collect::<Vec<_>>(),
        lastname: vec![],
        bnum: 1u64,
        multisig: false,
        scalars: vec![],
        points: vec![],
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
    stkinfo: Vec<(CompressedRistretto,u64)>, //  pk, $
    // iptable: BiHashMap<NodeId,CompressedRistretto>, // ip, pk // this could be useful for later but not significantly different communication for this stuff
    txses: Vec<Vec<u8>>,
    lastblock: NextBlock,
    sigs: Vec<NextBlock>,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bnum: u64,
    multisig: bool,
    scalars: Vec<Scalar>,
    points: Vec<RistrettoPoint>,
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
                    if (mtype == 2) | (mtype == 4) | (mtype == 6) {print!("#{:?}", mtype);}
                    else {println!("# MESSAGE TYPE: {:?}", mtype);}

                    if mtype == 0 {
                        self.txses.push(m[..std::cmp::min(m.len(),10_000)].to_vec());
                    } else if mtype == 2 {
                        self.sigs.push(bincode::deserialize(&m).unwrap());
                    } else if mtype == 4 {
                        self.points.push(CompressedRistretto(m.try_into().unwrap()).decompress().unwrap());
                    } else if mtype == 6 {
                        self.scalars.push(Scalar::from_bits(m.try_into().unwrap()));
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
            if (self.sigs.len() > (2*(NUMBER_OF_VALIDATORS/3)).into()) | ( (self.sigs.len() > (NUMBER_OF_VALIDATORS/2).into()) & (self.timekeeper.elapsed().as_secs() > 30) ) {
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
                    self.lastblock.scan_as_noone(&mut self.stkinfo,&self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>(), &mut self.queue, &mut self.exitqueue, &mut self.comittee, true);
                    
                    
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
            if (self.multisig == true) & (self.timekeeper.elapsed().as_secs() > 1) & (self.points.len() > (2*NUMBER_OF_VALIDATORS as usize)/3) {
                // should prob check that validators are accurate here?
                let mut m = MultiSignature::sum_group_x(&self.points).as_bytes().to_vec();
                m.push(5u8);
                self.inner.broadcast(m);
                self.points = vec![RISTRETTO_BASEPOINT_POINT,MultiSignature::sum_group_x(&self.points).decompress().unwrap()];
                did_something = true;

            }
            if (self.multisig == true) & (self.timekeeper.elapsed().as_secs() > 2) & (self.points.get(0) == Some(&RISTRETTO_BASEPOINT_POINT)) & (self.scalars.len() > NUMBER_OF_VALIDATORS as usize/2) {
                self.multisig = false; // should definitely check that validators are accurate here

                // this is for if everyone signed... really > 0.5 or whatever... 
                
                let failed_validators = vec![]; // need an extra round to weed out liers
                let mut lastblock = NextBlock::default();
                lastblock.bnum = self.bnum;
                lastblock.emptyness = MultiSignature{x: self.points[1].compress(), y: MultiSignature::sum_group_y(&self.scalars), pk: failed_validators};
                lastblock.last_name = self.lastname.clone();
                lastblock.pools = vec![0u16];

                
                let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(),self.lastname.clone(),bincode::serialize(&lastblock.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                let mut s = Sha3_512::new();
                s.update(&m);
                let leader = Signature::sign(&self.key, &mut s,&self.keylocation);
                lastblock.leader = leader;


                let mut m = bincode::serialize(&lastblock).unwrap();
                
                self.lastblock = lastblock;
                let mut hasher = Sha3_512::new();
                hasher.update(&m);
                m.push(3u8);
                self.inner.broadcast(m);

                self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                self.points = vec![];
                self.scalars = vec![];
                self.sigs = vec![];

                let m = bincode::serialize(&self.lastblock).unwrap();
                let mut hasher = Sha3_512::new();
                hasher.update(&m);
                self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                self.bnum += 1;

                self.timekeeper = Instant::now();
                did_something = true;

            }
            if /*(self.txses.len() > 0) |*/ (self.timekeeper.elapsed().as_secs() > 10) /*(self.txses.len() >= 512) | (self.timekeeper.elapsed().as_secs() > 60/self.bnum + 1)*/ { // make this floating point too for time
                self.sigs = vec![];
                self.points = vec![];
                self.scalars = vec![];
                let mut m = bincode::serialize(&self.txses).unwrap();
                m.push(1u8);
                self.inner.broadcast(m);
                self.txses = vec![];
                self.timekeeper = Instant::now();
                if self.txses.len() == 0 {self.multisig = true;}
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
