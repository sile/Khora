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


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::time::Instant;
use kora::transaction::*;
use curve25519_dalek::ristretto::CompressedRistretto;
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::validation::*;
use kora::constants::PEDERSEN_H;
use kora::ringmaker::*;


 // cargo run --bin chat --release 6060 
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
    let addr: SocketAddr = track_any_err!(format!("192.168.0.101:{}", port).parse())?; // wafstampede
    // let addr: SocketAddr = track_any_err!(format!("172.20.10.3:{}", port).parse())?; // my iphone
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.1:{}", port).parse())?;
    

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

    let leader = Account::new(&format!("{}",0)).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress(); //make a new account





    let (message_tx, message_rx) = mpsc::channel();




    let me = Account::new(&format!("{}",0));
    println!("{:?}",me.name());
    println!("{:?}",me.stake_acc().name());
    let mut node = UserNode {
        inner: node,
        me: me,
        message_rx: message_rx,
        mine: vec![],
        smine: vec![],
        alltagsever: vec![],
        stkinfo: vec![(leader,1u64)],
        lastblock: NextBlock::default(),
        queue: (0..max_shards).map(|_|[0usize;NUMBER_OF_VALIDATORS as usize].into_par_iter().collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|vec![0usize;NUMBER_OF_VALIDATORS as usize]).collect::<Vec<_>>(),
        lastname: vec![],
        height: 0u64,
        sheight: 0u64,
    };
    if node.stkinfo[0].0 == me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress() {
        node.smine = vec![[0u64,1u64]];
        println!("hey i guess i founded this crypto!");
    }
    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            println!("line sent: {:?}",line);
            let line = if let Ok(line) = line {
                line
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

struct UserNode {
    inner: Node<Vec<u8>>,
    me: Account,
    message_rx: mpsc::Receiver<String>,
    mine: Vec<(u64, OTAccount)>,
    smine: Vec<[u64; 2]>,
    alltagsever: Vec<CompressedRistretto>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    lastblock: NextBlock,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    height: u64,
    sheight: u64,
}
impl Future for UserNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            while let Async::Ready(Some(m)) = track_try_unwrap!(self.inner.poll()) {
                let mut m = m.payload().to_vec();
                let mtype = m.pop().unwrap();
                println!("# MESSAGE TYPE: {:?}", mtype);
                if mtype == 3 {
                    let mut hasher = Sha3_512::new();
                    hasher.update(&m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                    self.lastblock = bincode::deserialize(&m).unwrap();
                    if self.lastblock.last_name == self.lastname {
                        self.lastblock.scan_as_noone(&mut self.stkinfo,&self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>(), &mut self.queue, &mut self.exitqueue, &mut self.comittee);
                        for i in 0..self.comittee.len() {
                            select_stakers(&self.lastname, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                        }
                        self.lastblock.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                        self.lastblock.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee[0].par_iter().map(|x|*x as u64).collect::<Vec<_>>());
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

            while let Async::Ready(Some(m)) = self.message_rx.poll().expect("Never fails") {
                println!("# MESSAGE (sent): {:?}", m);
                let mut m = str::to_ascii_lowercase(&m).as_bytes().to_vec();
                let txtype = m.pop().unwrap();
                let mut outs = vec![];
                while m.len() > 0 {
                    let mut pks = vec![];
                    for _ in 0..3 {
                        let h1 = m.par_drain(..32).collect::<Vec<_>>().par_iter().map(|x| (x-97)).collect::<Vec<_>>();
                        let h2 = m.par_drain(..32).collect::<Vec<_>>().par_iter().map(|x| (x-97)*16).collect::<Vec<_>>();
                        pks.push(CompressedRistretto(h1.into_par_iter().zip(h2).map(|(x,y)|x+y).collect::<Vec<u8>>().try_into().unwrap()));
                    }
                    let x: [u8;8] = m.par_drain(..8).map(|x| (x-48)).collect::<Vec<_>>().try_into().unwrap();
                    // println!("hi {:?}",x);
                    let x = u64::from_le_bytes(x);
                    println!("amounts {:?}",x);
                    // println!("ha {:?}",1u64.to_le_bytes());
                    let amnt = Scalar::from(x);
                    let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                    outs.push((recv,amnt));
                }

// gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a
// gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a
                // println!("{:?}",outs);


                let mut txbin: Vec<u8>;
                if txtype == 33 /* ! */ {
                    let (loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.par_iter().map(|x|(x.0 as u64,x.1.clone())).unzip();

                    let rname = generate_ring(&loc.par_iter().map(|x|*x as usize).collect::<Vec<_>>(), &15, &self.height);
                    let ring = recieve_ring(&rname);
                    /* this is where people send you the ring members */ 
                    let mut rlring = ring.into_par_iter().map(|x| OTAccount::summon_ota(&History::get(&x))).collect::<Vec<OTAccount>>();
                    /* this is where people send you the ring members */ 
                    let me = self.me;
                    rlring.par_iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x.clone()) {*x = y; println!("{:?}",x.tag);});
                    let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                    let tx = tx.polyform(&rname);
                    tx.verify().unwrap();
                    txbin = bincode::serialize(&tx).unwrap();
                }
                else {
                    let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.par_iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                    let i = txtype as usize - 97usize;
                    let b = self.me.derive_stk_ot(&Scalar::from(amnt[i]));
                    let tx = Transaction::spend_ring(&vec![self.me.receive_ot(&b).unwrap()], &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                    let tx = tx.polyform(&loc[i].to_le_bytes().to_vec());
                    tx.verifystk(&self.stkinfo).unwrap();
                    txbin = bincode::serialize(&tx).unwrap();
                }
                txbin.push(0);

                println!("{:?}",txbin);
                self.inner.broadcast(txbin);
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
