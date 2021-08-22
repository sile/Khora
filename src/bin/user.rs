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
use std::collections::{HashMap, VecDeque};
use std::convert::TryInto;
use std::time::{Instant, Duration};
use kora::transaction::*;
use curve25519_dalek::ristretto::CompressedRistretto;
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::validation::*;
use kora::constants::PEDERSEN_H;
use kora::ringmaker::*;



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
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.101:{}", port).parse())?; // wafstampede
    // let addr: SocketAddr = track_any_err!(format!("172.20.10.3:{}", port).parse())?; // my iphone
    // let addr: SocketAddr = track_any_err!(format!("192.168.0.1:{}", port).parse())?;
    
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

    let leader = Account::new(&format!("{}",0)).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress(); //make a new account





    let (message_tx, message_rx) = mpsc::channel();




    let me = Account::new(&format!("{}",0));
    let mut node = UserNode {
        inner: node,
        me: me,
        message_rx: message_rx,
        mine: vec![],
        rname: vec![],
        rmems: HashMap::new(),
        smine: vec![],
        alltagsever: vec![],
        stkinfo: vec![(leader,1u64)],
        lastblock: NextBlock::default(),
        queue: (0..max_shards).map(|_|[0usize;NUMBER_OF_VALIDATORS as usize].into_par_iter().collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        exitqueue: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
        comittee: (0..max_shards).map(|_|vec![0usize;NUMBER_OF_VALIDATORS as usize]).collect::<Vec<_>>(),
        lastname: vec![],
        height: 0u64,
        sheight: 1u64,
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
    rname: Vec<u8>,
    rmems: HashMap<u64,OTAccount>,
    smine: Vec<[u64; 2]>, // [location, amount]
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

            while let Async::Ready(Some(msg)) = track_try_unwrap!(self.inner.poll()) {
                let mut m = msg.payload().to_vec();
                if let Some(mtype) = m.pop() {
                    if mtype == 2 {print!("#{:?}", mtype);}
                    else {println!("# MESSAGE TYPE: {:?}", mtype);}
                    
                    if mtype == 3 {
                        println!("----------------------{}",m.len());
                        let mut hasher = Sha3_512::new();
                        hasher.update(&m);
                        self.lastblock = bincode::deserialize(&m).unwrap();
                        if self.lastblock.last_name == self.lastname {
                            println!("===============================\nyay!");
                            self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                            self.lastblock.scan_as_noone(&mut self.stkinfo,&self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>(), &mut self.queue, &mut self.exitqueue, &mut self.comittee);
                            for i in 0..self.comittee.len() {
                                select_stakers(&self.lastname, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                            }
                            self.lastblock.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                            self.lastblock.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee[0].par_iter().map(|x|*x as u64).collect::<Vec<_>>());
                        }
                    }
                    if mtype == 254 {
                        self.rmems.insert(u64::from_le_bytes(m[64..72].try_into().unwrap()),History::read_raw(&m));
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
                let istx = m.pop().unwrap();
                if istx == 33 /* ! */ {
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
/**/ /*
send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- 
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a!
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000b!
  send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   ------- 
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!

split stake:
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a!

gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a!
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a!


    VVVVVVVVVVV pump up the height VVVVVVVVVVV
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000a!


128.61.4.96:9876 128.61.4.96:9875?

send from non stake (!) to non stake (gf...ob) VVVVVVVVVVVVVVV
gfjmlieehekfdigbggapelbbhmneojphaaohaoikfihgghdkjmkicijcmjgpmaofkccgngcfmlfhjdnklngecejjpepepdnplemnilakijgddackcniigmnpnpdcgmnboidgodekoloapleeenjhchfmghbfcbfnagiclaljfeobinadhofcclghemfnlkob10000000!!

*/ //
                    let mut txbin: Vec<u8>;
                    if txtype == 33 /* ! */ {
                        // let (_loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.par_iter().map(|x|(x.0 as u64,x.1.clone())).unzip();

                        // let rname = generate_ring(&loc.par_iter().map(|x|*x as usize).collect::<Vec<_>>(), &15, &self.height);
                        let ring = recieve_ring(&self.rname);
                        /* this is where people send you the ring members */ 
                        // let mut rlring = ring.into_par_iter().map(|x| OTAccount::summon_ota(&History::get(&x))).collect::<Vec<OTAccount>>();
                        let rmems = &self.rmems;
                        let mut rlring = ring.par_iter().map(|x| rmems[x].clone()).collect::<Vec<OTAccount>>();
                        /* this is where people send you the ring members */ 
                        let me = self.me;
                        rlring.par_iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x.clone()) {*x = y;});
                        let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                        tx.verify().unwrap();
                        let tx = tx.polyform(&self.rname);
                        tx.verify().unwrap();
                        txbin = bincode::serialize(&tx).unwrap();
                    } else {
                        let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.par_iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                        let i = txtype as usize - 97usize;
                        let b = self.me.derive_stk_ot(&Scalar::from(amnt[i]));
                        let tx = Transaction::spend_ring(&vec![self.me.receive_ot(&b).unwrap()], &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                        tx.verify().unwrap();
                        let tx = tx.polyform(&loc[i].to_le_bytes().to_vec());
                        tx.verifystk(&self.stkinfo).unwrap();
                        txbin = bincode::serialize(&tx).unwrap();
                    }
                    txbin.push(0);

                    println!("----------------------------------------------------------------\n{:?}",txbin);
                    self.inner.broadcast(txbin);
                } else if istx == 63 /* ? */ {
                    // 128.61.4.96:9876 128.61.4.96:9875?

                    let m = String::from_utf8_lossy(&m);
                    let m = m.split(" ").collect::<Vec<&str>>();
                    // let addrs = m.chunks_exact(21).map(|x| track_any_err!(String::from_utf8_lossy(&x).parse()).unwrap()).collect::<Vec<SocketAddr>>();
                    let addrs = m.into_iter().map(|x| track_any_err!(x.parse()).unwrap()).collect::<Vec<SocketAddr>>();
                    


                    let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.par_iter().map(|x|(x.0 as u64,x.1.clone())).unzip();

                    // println!("loc: {:?}",loc);
                    // println!("acc: {:?}",acc);
                    // println!("height: {}",self.height);
                    for (i,j) in loc.iter().zip(acc) {
                        assert!(OTAccount::summon_ota(&History::get(&i)).pk == j.pk); // would not actually be able to make these tests on a real user
                        assert!(OTAccount::summon_ota(&History::get(&i)).com.com == j.com.com); // would not actually be able to make these tests on a real user
                        self.rmems.insert(*i,j);
                    }
                    // maybe have bigger rings than 5? it's a choice i dont forbid anything
                    self.rname = generate_ring(&loc.par_iter().map(|x|*x as usize).collect::<Vec<_>>(), &11, &self.height);
                    // println!("rname: {:?}",self.rname);
                    let ring = recieve_ring(&self.rname);
                    let ring = ring.into_par_iter().filter(|x| loc.par_iter().all(|y|x!=y)).collect::<Vec<_>>();
                    println!("ring:----------------------------------\n{:?}",ring);
                    // 192.168.000.101:09876
                    let alen = addrs.len();
                    for (i,r) in ring.iter().enumerate() {
                        let ip = NodeId::new(addrs[i%alen], LocalNodeId::new(0));
                        let mut r = r.to_le_bytes().to_vec();
                        r.push(u8::MAX);
                        self.inner.dm(r,&vec![ip],false);
                        std::thread::sleep(Duration::from_millis(10u64));
                    }

                } else if istx == 42 /* * */ { // todo: THIS FAILS FOR SOME IPs
                    // 192.168.000.101:09876 192.168.000.101:09875*
                    // 172.020.010.014:09876 172.020.010.014:09875*
                    let addrs = m.chunks(22).map(|x| track_any_err!(String::from_utf8_lossy(&x[..21]).parse()).unwrap()).collect::<Vec<SocketAddr>>();
                    for a in addrs.into_iter() {
                        let ip = NodeId::new(a, LocalNodeId::new(0));
                        self.inner.dm(vec![],&vec![ip],true);
                    }

                } else if istx == 105 /* i */ {
                    println!("\nmy name:\n---------------------------------------------\n{:?}\n",self.me.name());
                    println!("\nmy addr plumtree:\n---------------------------------------------\n{:?}\n",self.inner.plumtree_node().id());
                    println!("\nmy addr hyparview:\n---------------------------------------------\n{:?}\n",self.inner.hyparview_node().id());
                    println!("\nmy staker name:\n---------------------------------------------\n{:?}\n",self.me.stake_acc().name());
                    let scalarmoney = self.mine.iter().map(|x|self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>();
                    println!("\nmy scalar money:\n---------------------------------------------\n{:?}\n",scalarmoney);
                    let moniez = u64::from_le_bytes(scalarmoney.as_bytes()[..8].try_into().unwrap());
                    println!("\nmy money:\n---------------------------------------------\n{:?}\n",moniez);
                    println!("\nmy money locations:\n---------------------------------------------\n{:?}\n",self.mine.iter().map(|x|x.0 as u64).collect::<Vec<_>>());
                    let stake = self.smine.iter().map(|x|x[1]).collect::<Vec<_>>();
                    println!("\nmy stake:\n---------------------------------------------\n{:?}\n",stake);
                    println!("\nheight:\n---------------------------------------------\n{:?}\n",self.height);
                    println!("\nsheight:\n---------------------------------------------\n{:?}\n",self.sheight);
                } else if istx == 98 /* b */ {
                    println!("\nlast block:\n---------------------------------------------\n{:#?}\n",self.lastblock);
                } else if istx == 97 /* a */ { // just for me
                    // let m = "172.020.010.014:09876 172.020.010.014:09875*".as_bytes().to_vec();
                    // let m = "192.168.000.101:09876 192.168.000.101:09875*".as_bytes().to_vec();
                    // let m = "128.061.004.096:09876 128.061.004.096:09875*".as_bytes().to_vec();
                    // "128.61.4.96:9876".parse::<SocketAddr>().unwrap();
                    // println!("parsed!");
                    // let m = "128.061.004.096:09876 128.061.004.096:09875".to_string();
                    // let addrs = m.split(&" ".to_string()).collect::<Vec<&str>>().into_iter().map(|x| {
                    //     x.parse().unwrap()
                    // }).collect::<Vec<SocketAddr>>();
                    // let m = "128.061.004.096:09876 128.061.004.096:09875".to_string();
                    let addrs = vec!["128.61.4.96:9876".parse::<SocketAddr>().unwrap(),"128.61.4.96:9875".parse::<SocketAddr>().unwrap()];
                    println!("{:?}",addrs);
                    for a in addrs.into_iter() {
                        let ip = NodeId::new(a, LocalNodeId::new(0));
                        self.inner.dm(vec![],&vec![ip],true);
                    }

                }
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}
