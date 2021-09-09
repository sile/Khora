#[macro_use]
extern crate clap;
#[macro_use]
extern crate trackable;

use clap::Arg;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::fs::File;
use std::io::{Read, Write};
use std::net::SocketAddr;
use trackable::error::MainError;


use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use kora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use kora::constants::PEDERSEN_H;
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::bloom::*;
use kora::validation::*;
use kora::ringmaker::*;
use serde::{Serialize, Deserialize};
use kora::validation::{NUMBER_OF_VALIDATORS, SIGNING_CUTOFF, QUEUE_LENGTH, REPLACERATE};

use local_ipaddress;

pub fn hash_to_scalar<T: Serialize> (message: &T) -> Scalar {
    let message = bincode::serialize(message).unwrap();
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
} /* this is for testing purposes. it is used to check if 2 long messages are identicle */

const WARNINGTIME: usize = REPLACERATE*5;

fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("PORT").index(1).required(true))
        .arg(Arg::with_name("PASSWORD").index(2).required(true))
        .arg(Arg::with_name("SAVE_HISTORY").index(3).default_value("1").required(false))
        .arg(Arg::with_name("CONTACT_SERVER").index(4).required(false))
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
    let pswrd = matches.value_of("PASSWORD").unwrap();
    
    let addr: SocketAddr = track_any_err!(format!("{}:{}", local_ipaddress::get().unwrap(), port).parse())?;
    println!("addr: {:?}",addr);
    println!("pswrd: {:?}",pswrd);


    let max_shards = 64usize; /* this if for testing purposes... there IS NO MAX SHARDS */
    
    // fs::remove_dir_all("blocks").unwrap(); // this would obviously not be used in the final version
    // fs::create_dir_all("blocks").unwrap();


    let executor = track_any_err!(ThreadPoolExecutor::new())?;
    let service = ServiceBuilder::new(addr)
        .logger(logger.clone())
        .finish(executor.handle(), SerialLocalNodeIdGenerator::new()); // everyone is node 0 rn... that going to be a problem? I mean everyone has different ips...
        
    let mut backnode = NodeBuilder::new().logger(logger.clone()).finish(service.handle());
    println!("{:?}",backnode.id());
    if let Some(contact) = matches.value_of("CONTACT_SERVER") {
        let contact: SocketAddr = track_any_err!(format!("{}:{}", local_ipaddress::get().unwrap(), contact).parse())?;
        println!("contact: {:?}",contact);
        backnode.join(NodeId::new(contact, LocalNodeId::new(0)));
    }
    let frontnode = NodeBuilder::new().logger(logger).finish(service.handle()); // todo: make this local_id random so people can't guess you
    println!("{:?}",frontnode.id()); // this should be the validator survice




    let (message_tx, message_rx) = mpsc::channel();



    let node: StakerNode;
    if pswrd != "load" {
        let leader = Account::new(&format!("{}","pig")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        // let initial_history = vec![(leader,1u64)];
        let otheruser = Account::new(&format!("{}","dog")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        let user3 = Account::new(&format!("{}","cow")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        let user4 = Account::new(&format!("{}","ant")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        let initial_history = vec![(leader,1u64),(otheruser,1u64),(user3,1u64),(user4,1u64)];


        let me = Account::new(&format!("{}",pswrd));
        let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
        let key = validator.sk.unwrap();
        let mut keylocation = HashSet::new();

        History::initialize();
        BloomFile::initialize_bloom_file();
        let bloom = BloomFile::from_keys(1, 2); // everyone has different keys for this

        let mut smine = vec![];
        for i in 0..initial_history.len() {
            if initial_history[i].0 == me.stake_acc().derive_stk_ot(&Scalar::from(initial_history[i].1)).pk.compress() {
                smine.push([i as u64,initial_history[i].1]);
                keylocation.insert(i as u64);
                println!("\n\nhey i guess i founded this crypto!\n\n");
            }

        }


        node = StakerNode {
            inner: frontnode,
            outer: backnode,
            message_rx: message_rx,
            save_history: (matches.value_of("SAVE_HISTORY").unwrap() != "0"),
            me: me,
            mine: vec![],
            smine: smine, // [location, amount]
            key: key,
            keylocation: keylocation,
            leader: leader,
            overthrown: HashSet::new(),
            votes: vec![0;NUMBER_OF_VALIDATORS],
            stkinfo: initial_history.clone(),
            lastblock: NextBlock::default(),
            queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>(),
            lastname: Scalar::one().as_bytes().to_vec(),
            bloom: bloom,
            bnum: 0u64,
            lastbnum: 0u64,
            height: 0u64,
            sheight: 1u64,
            alltagsever: vec![],
            txses: vec![],
            sigs: vec![],
            bannedlist: HashSet::new(),
            points: HashMap::new(),
            groupxnonce: 0,
            scalars: HashMap::new(),
            timekeeper: Instant::now(),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            stepeven: false,
            clogging: 0,
            emitmessage: Instant::now(),
            randomstakers: VecDeque::new(),
            laststkgossip: HashSet::new(),
            headshard: 0,
            usurpingtime: Instant::now(),
            is_validator: false,
            is_staker: true,
            sent_onces: HashSet::new(),
            knownvalidators: HashMap::new(),
            announcevalidationtime: Instant::now() - Duration::from_secs(10),
        };
    } else {
        node = StakerNode::load(frontnode, backnode, message_rx);
    }


    executor.spawn(service.map_err(|e| panic!("{}", e)));
    executor.spawn(node);


    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            // println!("line sent: {:?}",line);
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


#[derive(Clone, Serialize, Deserialize, Debug)]
struct SavedNode {
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: Vec<(u64, OTAccount)>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
    leader: CompressedRistretto,
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    lastblock: NextBlock,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: [u128;2],
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
    alltagsever: Vec<CompressedRistretto>,
    headshard: usize,
    view: Vec<SocketAddr>,
}

struct StakerNode {
    inner: Node<Vec<u8>>, // for sending and recieving messages as a validator (as in inner sanctum)
    outer: Node<Vec<u8>>, // for sending and recieving messages as a non validator (as in not inner)
    message_rx: mpsc::Receiver<String>,
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: Vec<(u64, OTAccount)>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
    leader: CompressedRistretto, // would they ever even reach consensus on this for new people when a dishonest person is eliminated???
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
    lastblock: NextBlock,
    queue: Vec<VecDeque<usize>>,
    exitqueue: Vec<VecDeque<usize>>,
    comittee: Vec<Vec<usize>>,
    lastname: Vec<u8>,
    bloom: BloomFile,
    bnum: u64,
    lastbnum: u64,
    height: u64,
    sheight: u64,
    alltagsever: Vec<CompressedRistretto>,
    txses: Vec<Vec<u8>>,
    sigs: Vec<NextBlock>,
    bannedlist: HashSet<NodeId>,
    points: HashMap<usize,RistrettoPoint>, // supplier, point
    groupxnonce: u64,
    scalars: HashMap<usize,Scalar>,
    timekeeper: Instant,
    waitingforentrybool: bool,
    waitingforleaderbool: bool,
    waitingforleadertime: Instant,
    waitingforentrytime: Instant,
    stepeven: bool,
    clogging: u64,
    emitmessage: Instant,
    laststkgossip: HashSet<Vec<u8>>,
    headshard: usize,
    usurpingtime: Instant,
    randomstakers: VecDeque<NodeId>,
    is_validator: bool,
    is_staker: bool,
    sent_onces: HashSet<Vec<u8>>,
    knownvalidators: HashMap<u64,NodeId>,
    announcevalidationtime: Instant,
}
impl StakerNode {
    fn save(&self) {
        let sn = SavedNode {
            save_history: self.save_history,
            me: self.me,
            mine: self.mine.clone(),
            smine: self.smine.clone(), // [location, amount]
            key: self.key,
            keylocation: self.keylocation.clone(),
            leader: self.leader.clone(),
            overthrown: self.overthrown.clone(),
            votes: self.votes.clone(),
            stkinfo: self.stkinfo.clone(),
            lastblock: self.lastblock.clone(),
            queue: self.queue.clone(),
            exitqueue: self.exitqueue.clone(),
            comittee: self.comittee.clone(),
            lastname: self.lastname.clone(),
            bloom: self.bloom.get_keys(),
            bnum: self.bnum,
            lastbnum: self.lastbnum,
            height: self.height,
            sheight: self.sheight,
            alltagsever: self.alltagsever.clone(),
            headshard: self.headshard.clone(),
            view: self.inner.hyparview_node().active_view().iter().map(|x| x.address()).collect::<Vec<_>>(),
        }; // just redo initial conditions on the rest
        let mut sn = bincode::serialize(&sn).unwrap();
        let mut f = File::create("myNode").unwrap();
        f.write_all(&mut sn).unwrap();
    }
    fn load(inner: Node<Vec<u8>>, outer: Node<Vec<u8>>, message_rx: mpsc::Receiver<String>,) -> StakerNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myNode").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();
        let mut inner = inner;
        inner.dm(vec![], &sn.view.iter().map(|&x| NodeId::new(x, LocalNodeId::new(0))).collect::<Vec<_>>(), true);
        StakerNode {
            inner: inner,
            outer: outer,
            message_rx,
            timekeeper: Instant::now(),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            emitmessage: Instant::now(),
            usurpingtime: Instant::now(),
            txses: vec![],
            sigs: vec![],
            bannedlist: HashSet::new(),
            points: HashMap::new(),
            scalars: HashMap::new(),
            laststkgossip: HashSet::new(),
            groupxnonce: 0,
            clogging: 0,
            stepeven: false,
            save_history: sn.save_history,
            me: sn.me,
            mine: sn.mine.clone(),
            smine: sn.smine.clone(), // [location, amount]
            key: sn.key,
            keylocation: sn.keylocation.clone(),
            leader: sn.leader.clone(),
            overthrown: sn.overthrown.clone(),
            votes: sn.votes.clone(),
            stkinfo: sn.stkinfo.clone(),
            lastblock: sn.lastblock.clone(),
            queue: sn.queue.clone(),
            exitqueue: sn.exitqueue.clone(),
            comittee: sn.comittee.clone(),
            lastname: sn.lastname.clone(),
            bloom: BloomFile::from_keys(sn.bloom[0],sn.bloom[1]),
            bnum: sn.bnum,
            lastbnum: sn.lastbnum,
            height: sn.height,
            sheight: sn.sheight,
            alltagsever: sn.alltagsever.clone(),
            headshard: sn.headshard.clone(),
            randomstakers: VecDeque::new(),
            is_validator: false,
            is_staker: true,
            sent_onces: HashSet::new(), // maybe occasionally clear this or replace with vecdeq?
            knownvalidators: HashMap::new(),
            announcevalidationtime: Instant::now() - Duration::from_secs(10),
        }
    }
}
impl Future for StakerNode {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;
            print!(".");

            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|\
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|--\
            \*/
            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|----\
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|----/
            \*/
            /*\_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner|--/
            \*/
            /*\control box for outer and inner_______________________________control box for outer and inner_______________________________control box for outer and inner_______________________________|/
            \*/
            if self.keylocation.iter().all(|keylocation| self.comittee[self.headshard].iter().all(|&x| x as u64 != *keylocation) ) { // if you're not in the comittee
                self.is_staker = true;
                self.is_validator = false;
            } else { // if you're in the comittee
                self.is_staker = false;
                self.is_validator = true;
                if (self.timekeeper.elapsed().as_secs() > 5) & self.keylocation.contains(&(self.comittee[self.headshard][0] as u64)) { // make this a floating point function for variable time
                    self.sigs = vec![];
                    self.points = HashMap::new();
                    self.scalars = HashMap::new();
                    let m = bincode::serialize(&self.txses).unwrap();
                    let mut m = Signature::sign_message(&self.key, &m, &(self.comittee[self.headshard][0] as u64));
                    m.push(1u8);
                    self.inner.broadcast(m);
                    self.txses = vec![];
                    self.timekeeper = Instant::now();
                    // if self.txses.len() == 0 {self.multisig = true;}
                    did_something = true;
                }
            }
            if self.keylocation.iter().all(|&key| !self.queue[self.headshard].contains(&(key as usize))) {
                self.knownvalidators = HashMap::new(); // need to test this
            }
            self.keylocation.clone().iter().for_each(|keylocation| { // get these numbers to be based on something
                if !self.queue[self.headshard].range(WARNINGTIME-REPLACERATE..WARNINGTIME).collect::<Vec<_>>().iter().all(|&&x| x as u64 != *keylocation) {
                    self.is_staker = true;
                    let message = bincode::serialize(self.outer.plumtree_node().id()).unwrap();
                    if self.sent_onces.insert(message.clone().into_iter().chain(self.bnum.to_le_bytes().to_vec().into_iter()).collect::<Vec<_>>()) {
                        println!("broadcasting name!");
                        let mut evidence = Signature::sign_message(&self.key, &message, keylocation);
                        evidence.push(118); // v
                        self.outer.broadcast(evidence); // add a dm your transactions to this list section (also add them to your list of known validators if they are validators)
                    }
                } else if !self.queue[self.headshard].range(REPLACERATE..WARNINGTIME-REPLACERATE).collect::<Vec<_>>().iter().all(|&&x| x as u64 != *keylocation) {
                    self.is_staker = true;
                }
                if !self.queue[self.headshard].range(0..REPLACERATE).collect::<Vec<_>>().iter().all(|&&x| x as u64 != *keylocation) {
                    self.is_staker = true;
                    self.is_validator = true;
                    if self.announcevalidationtime.elapsed().as_secs() > 10 {
                        let message = bincode::serialize(self.inner.plumtree_node().id()).unwrap();
                        let mut evidence = Signature::sign_message(&self.key, &message, &keylocation);
                        evidence.push(118); // v
                        self.inner.dm_now(evidence,&self.knownvalidators.iter().filter_map(|(&location,node)| {
                            let node = node.with_id(1);
                            if self.comittee[self.headshard].contains(&(location as usize)) & !(self.inner.plumtree_node().all_push_peers().contains(&node) | (node == self.inner.plumtree_node().id)) {
                                println!("(((((((((((((((((((((((((((((((((((((((((((((((dm'ing validators)))))))))))))))))))))))))))))))))))))))))))))))))))))");
                                Some(node)
                            } else {
                                None
                            }
                        }).collect::<Vec<_>>(), true); // add a recieve new members section for validators (also maybe don't send to the whole list?)
                        self.announcevalidationtime = Instant::now(); // i dont want to do thiss... 2 that drain into eachother?
                    }
                }
            });












             /*\__________________________________________________________________________________________________________________________
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::\
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF|\
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::|/\
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF/\/\___________________________________
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::\/\/
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF|\/
        |--0| VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::|/
        |--0| ::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF::::::::::::::::VALIDATOR STUFF/
             \*/
            if self.is_validator {
                while let Async::Ready(Some(msg)) = track_try_unwrap!(self.inner.poll()) {
                    if !self.bannedlist.contains(&msg.id().node()) {
                        let mut m = msg.payload().to_vec();
                        if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a anyone except user
                            self.clogging += 1;
                            if mtype == 2 {print!("#{:?}", mtype);}
                            else {println!("# MESSAGE TYPE: {:?} FROM: {:?}", mtype,msg.id().node());}
                            // println!("# MESSAGE TYPE: {:?}", mtype); // i dont do anything with lightning blocks because im a staker


                            if mtype == 0 {
                                self.txses.push(m[..std::cmp::min(m.len(),10_000)].to_vec());
                            } else if mtype == 1 {
                                if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                    if who == self.comittee[self.headshard][0] as u64 {
                                        let m: Vec<Vec<u8>> = bincode::deserialize(&m).unwrap(); // come up with something better
                                        let m = m.into_par_iter().map(|x| bincode::deserialize(&x).unwrap()).collect::<Vec<PolynomialTransaction>>();

                                        for keylocation in &self.keylocation {
                                            let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                                            if m.txs.len() > 0 {
                                                println!("{:?}",m.txs.len());
                                                let mut m = bincode::serialize(&m).unwrap();
                                                m.push(2);
                                                for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                                    self.inner.broadcast(m.clone());
                                                    std::thread::sleep(Duration::from_millis(10u64));
                                                }
                                            } else if (m.txs.len() == 0) & (m.emptyness.is_none()){
                                                self.groupxnonce += 1;
                                                let m = MultiSignature::gen_group_x(&self.key, &self.groupxnonce, &self.bnum).as_bytes().to_vec();// add ,(self.headshard as u16).to_le_bytes().to_vec() to m
                                                let mut m = Signature::sign_message(&self.key, &m, keylocation);
                                                m.push(4);
                                                if !self.comittee[self.headshard].iter().all(|&x|x as u64 != *keylocation) {
                                                    println!("I'm sending a MESSAGE TYPE 4 to {:?}",self.inner.plumtree_node().all_push_peers());
                                                    self.inner.broadcast(m.clone());
                                                }
                                            }
                                        }
                                        self.waitingforentrybool = false;
                                        self.waitingforleaderbool = true;
                                        self.waitingforleadertime = Instant::now();
                                    }
                                }
                            } else if mtype == 2 {
                                self.sigs.push(bincode::deserialize(&m).unwrap());
                            } else if mtype == 3 {
                                let lastblock: NextBlock = bincode::deserialize(&m).unwrap();
                                if lastblock.bnum >= self.bnum {
                                    let com = self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
                                    println!("someone's sending block {} with name: {:?}",lastblock.bnum,lastblock.last_name);
                                    println!("names match up: {}",lastblock.last_name == self.lastname);
                                    if lastblock.last_name != self.lastname {
                                        println!("{:?}\n{:?}",self.lastname,lastblock.last_name);
                                    }
                                    println!("stkinfo: {:?}",self.stkinfo);
                                    match lastblock.verify(&com[lastblock.shards[0] as usize], &self.stkinfo) {
                                        Ok(_) => println!("block verified..."),
                                        Err(x) => println!("Error in block verification: {}",x),
                                    };
                                    if (lastblock.shards[0] as usize >= self.headshard) & (lastblock.last_name == self.lastname) & lastblock.verify(&com[lastblock.shards[0] as usize], &self.stkinfo).is_ok() {
                                        self.headshard = lastblock.shards[0] as usize;

                                        self.lastblock = lastblock;
                                        println!("=========================================================\nyay validator!");
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());

                                        for _ in self.bnum..self.lastblock.bnum { // add whole different scannings for empty blocks
                                            println!("I missed a block!");
                                            NextBlock::pay_self_empty(&self.bnum, &self.headshard, &self.comittee, &mut self.smine);
                                            NextBlock::pay_all_empty(&self.bnum, &self.headshard, &mut self.comittee, &mut self.stkinfo);

                                            self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                                            for i in 0..self.comittee.len() {
                                                select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                                            }
                                            self.bnum += 1;
                                        }
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                                        if (self.lastblock.txs.len() > 0) | (self.bnum - self.lastbnum > 4) {
                                            self.lastblock.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                                            self.lastblock.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee, &self.stkinfo);
                                            self.keylocation = self.smine.iter().map(|x| x[0]).collect();
                                            self.lastblock.scan_as_noone(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, self.save_history);

                                            let lightning = bincode::serialize(&self.lastblock.tolightning()).unwrap();
                                            println!("saving block...");
                                            let mut f = File::create(format!("blocks/b{}",self.lastblock.bnum)).unwrap();
                                            f.write_all(&m).unwrap(); // writing doesnt show up in blocks in vs code immediatly
                                            self.lastbnum = self.bnum;
                                            let mut hasher = Sha3_512::new();
                                            hasher.update(lightning);
                                            self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                                        } else {
                                            NextBlock::pay_self_empty(&self.bnum, &self.headshard, &self.comittee, &mut self.smine);
                                            NextBlock::pay_all_empty(&self.bnum, &self.headshard, &mut self.comittee, &mut self.stkinfo);
                                        }
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                                        self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                                        for i in 0..self.comittee.len() {
                                            select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                                        }
                                        self.bnum += 1;
                                        

                                        
                                        // println!("-----------------------------------------------\n{}\n--------------------------------",self.comittee[self.headshard][self.exitqueue[self.headshard][0]]);
                                        // println!("{:?}",self.keylocation);
                                        println!("exitqueue: {:?}",self.exitqueue[self.headshard]);
                                        if self.keylocation.contains(&(self.comittee[self.headshard][self.exitqueue[self.headshard][0]] as u64)) | self.keylocation.contains(&(self.comittee[self.headshard][self.exitqueue[self.headshard][1]] as u64)) {
                                            m.push(3);
                                            println!("-----------------------------------------------\nsending out the new block {}!\n-----------------------------------------------",self.lastblock.bnum);
                                            self.outer.broadcast_now(m); /* broadcast the block to the outside world */
                                        }
            

                                        if self.lastblock.emptyness.is_some() {
                                            self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z - self.lastblock.emptyness.clone().unwrap().pk.iter().filter(|&&y| self.comittee[self.headshard][y as usize] == x).count() as i32).collect::<Vec<_>>();
                                        } else {
                                            self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z + self.lastblock.validators.clone().unwrap().iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                                        }
                                        
                                        /* LEADER CHOSEN BY VOTES */
                                        let mut abouttoleave = self.exitqueue[self.headshard].clone();
                                        let abouttoleave = abouttoleave.drain(..10).collect::<Vec<_>>().into_iter().map(|z| self.comittee[self.headshard][z].clone()).collect::<HashSet<_>>();
                                        self.leader = self.stkinfo[*self.comittee[self.headshard].iter().zip(self.votes.iter()).max_by_key(|(x,&y)| {
                                            if abouttoleave.contains(x) {
                                                i32::MIN
                                            } else {
                                                y
                                            }
                                        }).unwrap().0].0;
                                        /* LEADER CHOSEN BY VOTES */
                                        
                                        println!("block {} name: {:?}",self.bnum, self.lastname);

                                        self.stepeven = false;
                                        self.sigs = vec![];
                                        self.points = HashMap::new();
                                        self.scalars = HashMap::new();
                                        self.waitingforentrybool = true;
                                        self.waitingforleaderbool = false;
                                        self.waitingforleadertime = Instant::now();
                                        self.waitingforentrytime = Instant::now();
                                        self.timekeeper = Instant::now();
                                        self.usurpingtime = Instant::now();
                                    }
                                }
                                // println!("{:?}",hash_to_scalar(&self.lastblock));
                            } else if mtype == 4 {
                                if let Some(pk) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                    let pk = pk as usize;
                                    // println!("got sent from: {:?}",pk);
                                    if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                        println!("points getting bigger");
                                        self.points.insert(pk,CompressedRistretto(m.try_into().unwrap()).decompress().unwrap());
                                    }
                                }
                            } else if mtype == 5 {
                                let xt = CompressedRistretto(m.try_into().unwrap());
                                let mut mess = self.leader.as_bytes().to_vec();
                                mess.extend(&self.lastname);
                                mess.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                                // println!("from the me: {:?}",mess);
                                println!("you're trying to send a scalar!");
                                for keylocation in self.keylocation.iter() {
                                    let mut m = Signature::sign_message(&self.key, &MultiSignature::try_get_y(&self.key, &self.groupxnonce, &self.bnum, &mess, &xt).as_bytes().to_vec(), keylocation);
                                    m.push(6u8);
                                    if !self.comittee[self.headshard].iter().all(|&x| !self.keylocation.contains(&(x as u64))) {
                                        self.inner.broadcast(m.clone());
                                    }
                                }
                                self.waitingforleadertime = Instant::now();
                            } else if mtype == 6 {
                                // println!("someone's trying to send you a scalar!");
                                if let Some(pk) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                    let pk = pk as usize;
                                    println!("someone's REALLY trying to send you a scalar!");
                                    if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                        self.scalars.insert(pk,Scalar::from_bits(m.try_into().unwrap()));
                                    }
                                }
                            }
                        }
                    }
                    did_something = true;
                }
                /*_________________________________________________________________________________________________________
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||||
                ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF ||||||||||||| LEADER STUFF|
                *//////////////////////////////////////////////////////////////////////////////////////////////////////////
                // if self.headshard != 0 { // that tests shard usurption
                if self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress() == self.leader { // the computation for my stake key doesn't need to be done every time in the loop
                    if self.sigs.len() > SIGNING_CUTOFF /*| ( (self.sigs.len() > 64) & (self.timekeeper.elapsed().as_secs() > 30) )*/ {
                        let lastblock = NextBlock::finish(&self.key, &self.keylocation.iter().next().unwrap(), &self.sigs.drain(..).collect::<Vec<_>>(), &self.comittee[self.headshard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(self.headshard as u16), &self.bnum, &self.lastname, &self.stkinfo);
        
                        if lastblock.validators.is_some() {
                            // self.lastblock = lastblock;
        
                            let mut m = bincode::serialize(&lastblock).unwrap();
                            let l = bincode::serialize(&lastblock.tolightning()).unwrap();
            
                            self.sigs = vec![];
        
                            let mut hasher = Sha3_512::new();
                            hasher.update(&l);
                            // self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                            // println!("{:?}",hash_to_scalar(&self.lastblock));
        
        
            
        
                            // l.push(7u8);
                            // self.inner.broadcast(l);
                            m.push(3u8);
                            self.inner.broadcast(m);
        
        
                            
        
                            println!("made a block with {} transactions!",self.lastblock.txs.len());
                            did_something = true;
                        } else {
                            println!("failed to make a block :(");
                            did_something = false;
                        }
        
                        self.timekeeper = Instant::now();
                    }
                    if !self.stepeven & (self.timekeeper.elapsed().as_secs() > 1) & (self.comittee[self.headshard].iter().filter(|&x|self.points.contains_key(x)).count() > SIGNING_CUTOFF) {
                        // should prob check that validators are accurate here?
                        let points = self.points.par_iter().map(|x| *x.1).collect::<Vec<_>>();
                        let mut m = MultiSignature::sum_group_x(&points).as_bytes().to_vec();
                        m.push(5u8);
                        self.inner.broadcast(m);
                        // self.points = HashMap::new();
                        self.points.insert(usize::MAX,MultiSignature::sum_group_x(&points).decompress().unwrap());
                        self.stepeven = true;
                        did_something = true;
        
                    }
                    let k = self.scalars.keys().collect::<HashSet<_>>();
                    if self.stepeven & self.points.get(&usize::MAX).is_some() & (self.timekeeper.elapsed().as_secs() > 2) & (self.comittee[self.headshard].iter().filter(|x| k.contains(x)).count() > SIGNING_CUTOFF) {
                        let sumpt = self.points.remove(&usize::MAX).unwrap();
        
                        let keys = self.points.clone();
                        let mut keys = keys.keys().collect::<Vec<_>>();
                        let mut s = Sha3_512::new();
                        let mut m = self.leader.as_bytes().to_vec();
                        m.extend(&self.lastname);
                        m.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                        s.update(&m);
                        s.update(sumpt.compress().as_bytes());
                        let e = Scalar::from_hash(s);
                        // println!("keys: {:?}",keys);
                        let k = keys.len();
                        keys.retain(|&x| (self.points[x] + e*self.stkinfo[*x].0.decompress().unwrap() == self.scalars[x]*PEDERSEN_H()) & self.comittee[self.headshard].contains(x));
                        if k == keys.len() {

                            let failed_validators = vec![];
                            let mut lastblock = NextBlock::default();
                            lastblock.bnum = self.bnum;
                            lastblock.emptyness = Some(MultiSignature{x: sumpt.compress(), y: MultiSignature::sum_group_y(&self.scalars.values().map(|x| *x).collect::<Vec<_>>()), pk: failed_validators});
                            lastblock.last_name = self.lastname.clone();
                            lastblock.shards = vec![self.headshard as u16];
            
                            
                            let m = vec![BLOCK_KEYWORD.to_vec(),(self.headshard as u16).to_le_bytes().to_vec(),self.bnum.to_le_bytes().to_vec(),self.lastname.clone(),bincode::serialize(&lastblock.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                            let mut s = Sha3_512::new();
                            s.update(&m);
                            let leader = Signature::sign(&self.key, &mut s,&self.keylocation.iter().next().unwrap());
                            lastblock.leader = leader;
            
            
                            let mut m = bincode::serialize(&lastblock).unwrap();

                            println!("sending off block {}!!!",lastblock.bnum);
                            m.push(3u8);
                            self.inner.broadcast(m);
            
                            self.points = HashMap::new();
                            self.scalars = HashMap::new();
                            self.sigs = vec![];
            
                            
                            self.timekeeper = Instant::now();
        
                        } else { // need an extra round to weed out liers
                            let points = keys.iter().map(|x| self.points[x]).collect::<Vec<_>>();
                            let p = MultiSignature::sum_group_x(&points);
                            self.points.insert(usize::MAX,p.decompress().unwrap());
                            let mut m = p.as_bytes().to_vec();
                            m.push(5u8);
                            self.inner.broadcast(m);
                            self.points = keys.iter().map(|&x| (*x,self.points[x])).collect::<HashMap<_,_>>();
                            self.scalars = HashMap::new();
        
                            self.timekeeper -= Duration::from_secs(1);
                        }
                        self.stepeven = false;
                        did_something = true;
        
                    }
                }
                // }
                if (self.waitingforleadertime.elapsed().as_secs() > 30) & self.waitingforleaderbool {
                    self.waitingforleadertime = Instant::now();
                    /* change the leader, also add something about only changing the leader if block is free */

                    self.overthrown.insert(self.leader);
                    self.leader = self.stkinfo[*self.comittee[0].iter().zip(self.votes.iter()).max_by_key(|(&x,&y)| {
                        let candidate = self.stkinfo[x];
                        if self.overthrown.contains(&candidate.0) {
                            i32::MIN
                        } else {
                            y
                        }
                    }).unwrap().0].0;
                }
                if (self.waitingforentrytime.elapsed().as_secs() > 5) & self.waitingforentrybool {
                    self.waitingforentrybool = false;
                    let m = vec![];
                    for keylocation in &self.keylocation {
                        let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                        if m.txs.len() > 0 {
                            println!("{:?}",m.txs.len());
                            let mut m = bincode::serialize(&m).unwrap();
                            m.push(2);
                            for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                self.inner.broadcast(m.clone());
                                std::thread::sleep(Duration::from_millis(10u64));
                            }
                        } else if (m.txs.len() == 0) & (m.emptyness.is_none()){
                            self.groupxnonce += 1;
                            let m = MultiSignature::gen_group_x(&self.key, &self.groupxnonce, &self.bnum).as_bytes().to_vec();// add ,(self.headshard as u16).to_le_bytes().to_vec() to m
                            let mut m = Signature::sign_message(&self.key, &m, keylocation);
                            m.push(4);
                            if !self.comittee[self.headshard].iter().all(|&x|x as u64 != *keylocation) {
                                println!("I'm sending a MESSAGE TYPE 4 to {:?}",self.inner.plumtree_node().all_push_peers());
                                self.inner.broadcast(m.clone());
                            }
                        }
                    }

                }
            }
             /*\______________________________________________________________________________________________
        |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::\
        |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF|\
        |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::|/\
        |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF/\/\___________________________________
        |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::\/\/
        |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF|\/
        |--0| STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::|/
        |--0| ::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF::::::::::::STAKER STUFF/
             \*/
            if self.is_staker {
                while let Async::Ready(Some(msg)) = track_try_unwrap!(self.outer.poll()) {
                    if !self.bannedlist.contains(&msg.id().node()) {
                        let mut m = msg.payload().to_vec();
                        if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a anyone except user
                            self.clogging += 1;
                            println!("# MESSAGE TYPE: {:?}", mtype);


                            if mtype == 0 {
                                self.txses.push(m[..std::cmp::min(m.len(),10_000)].to_vec());
                            } else if (mtype == 3) & !self.is_validator {
                                let lastblock: NextBlock = bincode::deserialize(&m).unwrap();
                                // let mut hasher = Sha3_512::new();
                                // hasher.update(&m);
                                // self.lastblock = bincode::deserialize(&m).unwrap();
                                if lastblock.bnum >= self.bnum {
                                    let com = self.comittee.par_iter().map(|x|x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
                                    println!("someone's sending block {} with name: {:?}",lastblock.bnum,lastblock.last_name);
                                    println!("names match up: {}",lastblock.last_name == self.lastname);
                                    println!("stkinfo: {:?}",self.stkinfo);
                                    match lastblock.verify(&com[lastblock.shards[0] as usize], &self.stkinfo) {
                                        Ok(_) => println!("block verified..."),
                                        Err(x) => println!("Error in block verification: {}",x),
                                    };
                                    if (lastblock.shards[0] as usize >= self.headshard) & (lastblock.last_name == self.lastname) & lastblock.verify(&com[lastblock.shards[0] as usize], &self.stkinfo).is_ok() {
                                        self.headshard = lastblock.shards[0] as usize;

                                        self.lastblock = lastblock;
                                        println!("=========================================================\nyay staker!");
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());

                                        // self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();

                                        for _ in self.bnum..self.lastblock.bnum { // add whole different scannings for empty blocks
                                            println!("I missed a block!");
                                            NextBlock::pay_self_empty(&self.bnum, &self.headshard, &self.comittee, &mut self.smine);
                                            NextBlock::pay_all_empty(&self.bnum, &self.headshard, &mut self.comittee, &mut self.stkinfo);

                                            self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                                            for i in 0..self.comittee.len() {
                                                select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                                            }
                                            self.bnum += 1;
                                            // self.lastname = (Scalar::from_canonical_bytes(self.lastname.clone().try_into().unwrap()).unwrap() + Scalar::one()).as_bytes().to_vec();
                                        }
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                                        if (self.lastblock.txs.len() > 0) | (self.bnum - self.lastbnum > 4) {
                                            println!("running a full block!");
                                            self.lastblock.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever);
                                            println!("scanning as stk!");
                                            self.lastblock.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee, &self.stkinfo);
                                            self.keylocation = self.smine.iter().map(|x| x[0]).collect();
                                            println!("scanning as noone!");
                                            self.lastblock.scan_as_noone(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, self.save_history);

                                            let lightning = bincode::serialize(&self.lastblock.tolightning()).unwrap();
                                            println!("saving block...");
                                            let mut f = File::create(format!("blocks/b{}",self.lastblock.bnum)).unwrap();
                                            f.write_all(&m).unwrap(); // writing doesnt show up in blocks in vs code immediatly
                                            self.lastbnum = self.bnum;
                                            let mut hasher = Sha3_512::new();
                                            hasher.update(lightning);
                                            self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                                        } else {
                                            println!("running an empty block!");
                                            NextBlock::pay_self_empty(&self.bnum, &self.headshard, &self.comittee, &mut self.smine);
                                            NextBlock::pay_all_empty(&self.bnum, &self.headshard, &mut self.comittee, &mut self.stkinfo);
                                        }
                                        println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                                        self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                                        for i in 0..self.comittee.len() {
                                            select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                                        }
                                        self.bnum += 1;
                                        // self.lastname = (Scalar::from_canonical_bytes(self.lastname.clone().try_into().unwrap()).unwrap() + Scalar::one()).as_bytes().to_vec();
            

                                        if self.lastblock.emptyness.is_some() {
                                            self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z - self.lastblock.emptyness.clone().unwrap().pk.iter().filter(|&&y| self.comittee[self.headshard][y as usize] == x).count() as i32).collect::<Vec<_>>();
                                        } else {
                                            self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z + self.lastblock.validators.clone().unwrap().iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                                        }
                                        
                                        /* LEADER CHOSEN BY VOTES */
                                        let mut abouttoleave = self.exitqueue[self.headshard].clone();
                                        let abouttoleave = abouttoleave.drain(..10).collect::<Vec<_>>().into_iter().map(|z| self.comittee[self.headshard][z].clone()).collect::<HashSet<_>>();
                                        self.leader = self.stkinfo[*self.comittee[self.headshard].iter().zip(self.votes.iter()).max_by_key(|(x,&y)| {
                                            if abouttoleave.contains(x) {
                                                i32::MIN
                                            } else {
                                                y
                                            }
                                        }).unwrap().0].0;
                                        /* LEADER CHOSEN BY VOTES */
                                        
                                        println!("block {} name: {:?}",self.bnum, self.lastname);


                                        self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                                            if self.queue[self.headshard].contains(&(location as usize)) | self.comittee[self.headshard].contains(&(location as usize)) {
                                                Some((location,node))
                                            } else {
                                                None
                                            }
                                        }).collect::<HashMap<_,_>>();

                                        self.stepeven = false;
                                        self.timekeeper = Instant::now();
                                    }
                                }
                                // println!("{:?}",hash_to_scalar(&self.lastblock));
                            } else if (mtype == 105) & !self.is_validator /* i */ {
                                if Signature::recieve_signed_message(&mut m, &self.stkinfo).is_some() {
                                    self.randomstakers.push_front(bincode::deserialize::<NodeId>(&m).unwrap());
                                    self.randomstakers.pop_back();
                                }
                                // add identity to known people and delete oldest maybe (VecDeque)
                            } else if (mtype == 112) & !self.is_validator /* p */ {
                                self.laststkgossip.insert(m);
                            } else if (mtype == 114) & !self.is_validator /* r */ {
                                let mut y = m[..8].to_vec();
                                let mut x = History::get_raw(&u64::from_le_bytes(y.clone().try_into().unwrap())).to_vec();
                                x.append(&mut y);
                                x.push(254);
                                self.outer.dm(x,&vec![msg.id().node()],false);
                            } else if (mtype == 116) & !self.is_validator /* t */ { // this is totally untested
                                let tsk = Scalar::from_canonical_bytes(m.try_into().unwrap()).unwrap();
                                let mut location = 0u64;
                                let mut allyours = vec![];
                                for b in 0..=self.bnum {
                                    let file = format!("blocks/b{}",b);
                                    println!("checking for file {:?}...",file);
                                    if let Ok(mut file) = File::open(file) {
                                        let mut x = vec![];
                                        file.read_to_end(&mut x).unwrap();
                                        let block = bincode::deserialize::<NextBlock>(&x).unwrap().tolightning();
                                        println!("sending block {} of {}\t{:?}",b,self.bnum,block.last_name);
                                        let thisheight = block.info.txout.len() as u64;
                                        let yours = block.info.txout.par_iter().enumerate().filter_map(|(i,a)| {
                                            if a.track_ot(&tsk) {
                                                Some((i as u64 + location,a.clone()))
                                            } else {
                                                None
                                            }
                                        }).collect::<Vec<_>>();
                                        allyours.par_extend(yours);
                                        location += thisheight;
                                    }
                                }
                                self.outer.dm(bincode::serialize(&allyours).unwrap(),&vec![msg.id().node()],false);
                            } else if mtype == 118 /* v */ {
                                if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                    let m = bincode::deserialize::<NodeId>(&m).unwrap();
                                    if self.queue[self.headshard].contains(&(who as usize)) {
                                        self.knownvalidators.insert(who,m.with_id(0));
                                    }
                                    if self.comittee[self.headshard].contains(&(who as usize)) {
                                        self.inner.join(m.with_id(1));
                                    }
                                }
                            } else if (mtype == 121) & !self.is_validator /* y */ {
                                let theirnum = u64::from_le_bytes(m.try_into().unwrap());
                                println!("they're at {}, syncing them...",theirnum);
                                for b in theirnum+1..=self.bnum {
                                    let file = format!("blocks/b{}",b);
                                    println!("checking for file {:?}...",file);
                                    if let Ok(mut file) = File::open(file) {
                                        let mut x = vec![];
                                        file.read_to_end(&mut x).unwrap();
                                        println!("sending block {} of {}\t{:?}",b,self.bnum,bincode::deserialize::<NextBlock>(&x).unwrap().last_name);
                                        x.push(3);
                                        self.outer.dm(x,&vec![msg.id().node()],false);
                                    }
                                }
                            }
                        }
                    }
                    did_something = true;
                }
                /*____________________________________________________________________________________________________________________________________________________________________________________________________________________________
                RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
                ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION|
                RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
                ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION|
                RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
                ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION|
                RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
                ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION|
                RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION -------------------|
                ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION ------------------- RANDOM EMMISSION|
                */////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
                if self.emitmessage.elapsed().as_secs() > 300 {
                    self.emitmessage = Instant::now();
                    let mut m = hash_to_scalar(&self.stkinfo).as_bytes().to_vec();
                    if self.laststkgossip.contains(&m) & (self.clogging < 3000) { // 10 messages per second
                        m.push(112u8);
                        self.outer.broadcast(m);
                        let mut m = Signature::sign_message(&self.key, &bincode::serialize(&self.outer.id().address()).unwrap(), &self.keylocation.iter().next().unwrap());
                        m.push(105u8);
                        self.outer.broadcast(m);
                    }
                    self.laststkgossip = HashSet::new();
                    self.clogging = 0;
                }
            }










/*
send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- send to non stake ------- 
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000a!
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000b!
  send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   -------   send to stake   ------- 
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!

               VVVVVVVVVVVVVVVVVV split stake VVVVVVVVVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000a!

                  VVVVVVVVVVV pump up the height VVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigmnjimdgmpelmdoehiemiefmhinffkcnbmkjofflhfcpbcamfhheknjkibbcooeccgfemcpbnfommaiefmllkeekmghjokbhjepfgnfeilgjkipokjmfffggckekhpbef10000000a!


   VVVVVVVVVVVVVVV send from non stake (!) to non stake (gf...ob) VVVVVVVVVVVVVVV
ippcaamfollgjphmfpicoomjbphhepifhpkemhihaegcilmlkemajnolgocakhigccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoichccokkmobiejbfabpidlkfcnnggjfanngopkaglehkikgmafffoagkinilkfeoich10000000!!

*/






            /*_________________________________________________________________________________________________
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF |||||||||||||
            ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF ||||||||||||| USER STUFF
            */
            while let Async::Ready(Some(m)) = self.message_rx.poll().expect("Never fails") {
                if m.len() > 0 {
                    println!("# MESSAGE (sent): {:?}", m);
                    let mut m = str::to_ascii_lowercase(&m).as_bytes().to_vec();
                    let istx = m.pop().unwrap();
                    if istx == 33 /* ! */ {
                        let txtype = m.pop().unwrap();
                        let mut outs = vec![];
                        while m.len() > 0 {
                            let mut pks = vec![];
                            for _ in 0..3 { // read the pk address
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

                        let mut txbin: Vec<u8>;
                        if txtype == 33 /* ! */ {
                            let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.par_iter().map(|x|(x.0 as u64,x.1.clone())).unzip();

                            let rname = generate_ring(&loc.par_iter().map(|x|*x as usize).collect::<Vec<_>>(), &5, &self.height);
                            let ring = recieve_ring(&rname);
                            println!("ring: {:?}",ring);
                            println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                            println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                            let mut rlring = ring.into_iter().map(|x| {
                                let x = OTAccount::summon_ota(&History::get(&x));
                                if acc.par_iter().all(|a| a.pk != x.pk) {
                                    println!("not mine!");
                                    x
                                } else {
                                    println!("mine!");
                                    acc.par_iter().filter(|a| a.pk == x.pk).collect::<Vec<_>>()[0].to_owned()
                                }
                            }).collect::<Vec<OTAccount>>();
                            println!("ring len: {:?}",rlring.len());
                            let me = self.me;
                            rlring.par_iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x.clone()) {*x = y;});
                            let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            tx.verify().unwrap();
                            let tx = tx.polyform(&rname);
                            tx.verify().unwrap();
                            txbin = bincode::serialize(&tx).unwrap();
                        } else {
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.par_iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let i = txtype as usize - 97usize;
                            let b = self.me.derive_stk_ot(&Scalar::from(amnt[i]));
                            let tx = Transaction::spend_ring(&vec![self.me.receive_ot(&b).unwrap()], &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            tx.verify().unwrap();
                            println!("stkinfo: {:?}",self.stkinfo);
                            println!("me pk: {:?}",self.me.receive_ot(&b).unwrap().pk.compress());
                            println!("loc: {:?}",loc);
                            println!("amnt: {:?}",amnt);
                            let tx = tx.polyform(&loc[i].to_le_bytes().to_vec());
                            tx.verifystk(&self.stkinfo).unwrap();
                            txbin = bincode::serialize(&tx).unwrap();
                        }
                        txbin.push(0);

                        // println!("----------------------------------------------------------------\n{:?}",txbin);
                        self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                            if self.queue[self.headshard].contains(&(location as usize)) {
                                Some((location,node))
                            } else {
                                None
                            }
                        }).collect::<HashMap<_,_>>();
                        self.outer.broadcast(txbin);
                    } else if istx == 42 /* * */ { // ips to talk to
                        // 192.168.000.101:09876 192.168.000.101:09875*
                        let addrs = String::from_utf8_lossy(&m);
                        let addrs = addrs.split(" ").collect::<Vec<_>>().par_iter().map(|x| NodeId::new(x.parse::<SocketAddr>().unwrap(), LocalNodeId::new(0))).collect::<Vec<_>>();
                        self.outer.dm(vec![],&addrs,true);
                    } else if istx == 105 /* i */ {
                        println!("\nmy name:\n---------------------------------------------\n{:?}\n",self.me.name());
                        println!("\nmy outer addr:\n---------------------------------------------\n{:?}\n",self.outer.plumtree_node().id());
                        println!("\nmy inner addr:\n---------------------------------------------\n{:?}\n",self.inner.plumtree_node().id());
                        println!("\nmy staker name:\n---------------------------------------------\n{:?}\n",self.me.stake_acc().name());
                        let scalarmoney = self.mine.iter().map(|x|self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>();
                        println!("\nmy scalar money:\n---------------------------------------------\n{:?}\n",scalarmoney);
                        let moniez = u64::from_le_bytes(scalarmoney.as_bytes()[..8].try_into().unwrap());
                        println!("\nmy money:\n---------------------------------------------\n{:?}\n",moniez);
                        println!("\nmy money locations:\n---------------------------------------------\n{:?}\n",self.mine.iter().map(|x|x.0 as u64).collect::<Vec<_>>());
                        println!("\nmy stake:\n---------------------------------------------\n{:?}\n",self.smine);
                        println!("\nstake state:\n---------------------------------------------\n{:?}\n",self.stkinfo);
                        println!("\nheight:\n---------------------------------------------\n{:?}\n",self.height);
                        println!("\nsheight:\n---------------------------------------------\n{:?}\n",self.sheight);
                        println!("\ncomittee:\n---------------------------------------------\n{:?}\n",self.comittee[self.headshard]);
                        println!("\nleadership:\n---------------------------------------------\nmyself: {:?}\nleader: {:?}\n",self.me.stake_acc().derive_stk_ot(&Scalar::one()).pk.compress().as_bytes(), self.leader.as_bytes());
                        println!("\ntime:\n---------------------------------------------\n{:?}s\n",self.timekeeper.elapsed().as_secs());
                        println!("\nblock number:\n---------------------------------------------\n{:?}\n",self.bnum);
                        println!("\nblock name:\n---------------------------------------------\n{:?}\n",self.lastname);
                        println!("\ninner friends:\n---------------------------------------------\n{:?}\n",self.inner.plumtree_node().all_push_peers());
                        println!("\nouter friends:\n---------------------------------------------\n{:?}\n",self.outer.plumtree_node().all_push_peers());
                        // println!("\ninner eagers:\n---------------------------------------------\n{:?}\n",self.inner.plumtree_node().eager_push_peers());
                        // println!("\nouter eagers:\n---------------------------------------------\n{:?}\n",self.outer.plumtree_node().eager_push_peers());
                        println!("\nis validating:\n---------------------------------------------\n{:?}\n",self.is_validator);
                        println!("\nis validating:\n---------------------------------------------\n{:?}\n",self.is_validator);
                        println!("\nknown validators:\n---------------------------------------------\n{:?}\n",self.knownvalidators);
                    } else if istx == 115 /* s */ {
                        self.save();
                        // maybe do something else??? like save or load contacts???


                        // add sync request button and rotor and timer tp cycle through friends to sync in dm

                    } else if istx == 112 /* p */ { // exhausts action queues hopefully
                        for _ in 0..1000 {
                            self.inner.poll();
                            self.outer.poll();
                        }
                    } else if istx == 100 /* d */ {
                        let leader = Account::new(&format!("{}","pig")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
                        // let initial_history = vec![(leader,1u64)];
                        let otheruser = Account::new(&format!("{}","dog")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
                        let user3 = Account::new(&format!("{}","cow")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
                        let user4 = Account::new(&format!("{}","ant")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
                        let initial_history = vec![(leader,1u64),(otheruser,1u64),(user3,1u64),(user4,1u64)];
                        self.bnum = 0;
                        self.lastbnum = 0;
                        self.height = 0;
                        self.sheight = 1;
                        self.lastname = Scalar::one().as_bytes().to_vec();
                        self.lastblock = NextBlock::default();
                        self.queue = (0..self.comittee.len()).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>();
                        self.exitqueue = (0..self.comittee.len()).map(|_|(0..NUMBER_OF_VALIDATORS).collect::<VecDeque<usize>>()).collect::<Vec<_>>();
                        self.comittee = (0..self.comittee.len()).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>();
                        self.stkinfo = initial_history.clone();

                    } else if istx == 121 /* y */ {
                        let mut mynum = (self.bnum - 1).to_le_bytes().to_vec(); // remember the attack where you send someone middle blocks during gap
                        mynum.push(121);
                        let mut friend = self.outer.hyparview_node().active_view().to_vec();
                        friend.extend(self.outer.hyparview_node().passive_view().to_vec());
                        self.outer.dm(mynum, &friend, false); // you really dont need to send it to all your friends though
                    } else if istx == 98 /* b */ {
                        println!("\nlast block:\n---------------------------------------------\n{:#?}\n",self.lastblock);
                    } else if istx == 97 /* a */ { // 9876 9875a   (just input the ports, only for testing on a single computer)
                        let addrs = String::from_utf8_lossy(&m);
                        let addrs = addrs.split(" ").collect::<Vec<_>>().par_iter().map(|x| NodeId::new( format!("{}:{}", local_ipaddress::get().unwrap(), x).parse::<SocketAddr>().unwrap(), LocalNodeId::new(0))).collect::<Vec<_>>();

                        self.outer.dm(vec![],&addrs,true);
                    }
                }
                did_something = true;
            }


























            if self.usurpingtime.elapsed().as_secs() > 300 { // this will be much larger
                self.timekeeper = self.usurpingtime;
                self.usurpingtime = Instant::now();
                self.headshard += 1;
            }

        }
        Ok(Async::NotReady)
    }
}
