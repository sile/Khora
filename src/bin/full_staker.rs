#[macro_use]
extern crate clap;
#[macro_use]
extern crate trackable;

use clap::Arg;
use fibers::sync::mpsc;
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::{Async, Future, Poll, Stream};
use kora::seal::BETA;
use plumcast::node::{LocalNodeId, Node, NodeBuilder, NodeId, SerialLocalNodeIdGenerator};
use plumcast::service::ServiceBuilder;
use rand::prelude::SliceRandom;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::Build;
use std::fs::File;
use std::io::{Read, Write};
use std::net::SocketAddr;
use trackable::error::MainError;
use crossbeam::channel;


use kora::{account::*, gui};
use curve25519_dalek::scalar::Scalar;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use std::borrow::Borrow;
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

fn hash_to_scalar<T: Serialize> (message: &T) -> Scalar {
    let message = bincode::serialize(message).unwrap();
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
} /* this is for testing purposes. it is used to check if 2 long messages are identicle */

const WARNINGTIME: usize = REPLACERATE*5;
const BLANKS_IN_A_ROW: u64 = 60;
fn blocktime(cumtime: f64) -> f64 {
    // 60f64/(6.337618E-8f64*cumtime+2f64).ln()
    10.0
}
fn reward(cumtime: f64, blocktime: f64) -> f64 {
    (1.0/(1.653439E-6*cumtime + 1.0) - 1.0/(1.653439E-6*(cumtime + blocktime) + 1.0))*10E16f64
}
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



    let (ui_sender, urecv) = mpsc::channel();
    let (usend, ui_reciever) = channel::unbounded();



    let node: StakerNode;
    if pswrd != "load" {
        let leader = Account::new(&format!("{}","pig")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        // let initial_history = vec![(leader,1u64)];
        let otheruser = Account::new(&format!("{}","dog")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        let user3 = Account::new(&format!("{}","cow")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        let initial_history = vec![(leader,1u64),(otheruser,1u64),(user3,1u64)];
        // let user4 = Account::new(&format!("{}","ant")).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
        // let initial_history = vec![(leader,1u64),(otheruser,1u64),(user3,1u64),(user4,1u64)];


        let me = Account::new(&format!("{}",pswrd));
        let validator = me.stake_acc().receive_ot(&me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap(); //make a new account
        let key = validator.sk.unwrap();
        let mut keylocation = HashSet::new();

        History::initialize();
        BloomFile::initialize_bloom_file();
        let bloom = BloomFile::from_keys(1, 2); // everyone has different keys for this IMPORTANT TO CHANGR

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
            save_history: (matches.value_of("SAVE_HISTORY").unwrap() != "0"),
            me: me,
            mine: HashMap::new(),
            smine: smine.clone(), // [location, amount]
            key: key,
            keylocation: keylocation,
            leader: leader,
            overthrown: HashSet::new(),
            votes: vec![0;NUMBER_OF_VALIDATORS],
            stkinfo: initial_history.clone(),
            queue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            exitqueue: (0..max_shards).map(|_|(0..QUEUE_LENGTH).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)).collect::<VecDeque<usize>>()).collect::<Vec<_>>(),
            comittee: (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (x%NUMBER_OF_VALIDATORS)%initial_history.len()).collect::<Vec<usize>>()).collect::<Vec<_>>(),
            lastname: Scalar::one().as_bytes().to_vec(),
            bloom: bloom,
            bnum: 0u64,
            lastbnum: 0u64,
            height: 0u64,
            sheight: initial_history.len() as u64,
            alltagsever: vec![],
            txses: vec![],
            sigs: vec![],
            bannedlist: HashSet::new(),
            points: HashMap::new(),
            scalars: HashMap::new(),
            timekeeper: Instant::now(),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            doneerly: Instant::now(),
            headshard: 0,
            usurpingtime: Instant::now(),
            is_validator: false,
            is_staker: true,
            sent_onces: HashSet::new(),
            knownvalidators: HashMap::new(),
            announcevalidationtime: Instant::now() - Duration::from_secs(10),
            leaderip: None,
            newest: 0u64,
            rmems: HashMap::new(),
            rname: vec![],
            is_user: smine.is_empty(),
            gui_sender: usend,
            gui_reciever: urecv,
            moneyreset: None,
            sync_returnaddr: None,
            sync_theirnum: 0u64,
            sync_lightning: 'b',
            outs: None,
            groupsent: [false;2],
            oldstk: None,
            cumtime: 0f64,
            blocktime: blocktime(0.0),
        };
    } else {
        node = StakerNode::load(frontnode, backnode, usend, urecv);
    }
    let staked: String;
    if let Some(founder) = node.smine.get(0) {
        staked = format!("{}",founder[1]);
    } else {
        staked = "0".to_string();
    }


    println!("starting!");
    let app = gui::TemplateApp::new(
        ui_reciever,
        ui_sender,
        staked,
        node.me.name(),
        node.me.stake_acc().name(),
        pswrd.to_string(),
    );
    println!("starting!");
    let native_options = eframe::NativeOptions::default();
    println!("starting!");
    std::thread::spawn(move || {
        executor.spawn(service.map_err(|e| panic!("{}", e)));
        executor.spawn(node);


        track_any_err!(executor.run()).unwrap();
    });
    eframe::run_native(Box::new(app), native_options);
    println!("ending!");
    // add save node command somehow or add that as exit thing and add wait for saved signal here
    Ok(())
}


#[derive(Clone, Serialize, Deserialize, Debug)]
struct SavedNode {
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
    leader: CompressedRistretto,
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
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
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    is_user: bool,
    moneyreset: Option<Vec<u8>>,
    oldstk: Option<(Account, Vec<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
}

struct StakerNode {
    inner: Node<Vec<u8>>, // for sending and recieving messages as a validator (as in inner sanctum)
    outer: Node<Vec<u8>>, // for sending and recieving messages as a non validator (as in not inner)
    gui_sender: channel::Sender<Vec<u8>>,
    gui_reciever: mpsc::Receiver<Vec<u8>>,
    save_history: bool, //just testing. in real code this is true; but i need to pretend to be different people on the same computer
    me: Account,
    mine: HashMap<u64, OTAccount>,
    smine: Vec<[u64; 2]>, // [location, amount]
    key: Scalar,
    keylocation: HashSet<u64>,
    leader: CompressedRistretto, // would they ever even reach consensus on this for new people when a dishonest person is eliminated???
    overthrown: HashSet<CompressedRistretto>,
    votes: Vec<i32>,
    stkinfo: Vec<(CompressedRistretto,u64)>,
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
    scalars: HashMap<usize,Scalar>,
    timekeeper: Instant,
    waitingforentrybool: bool,
    waitingforleaderbool: bool,
    waitingforleadertime: Instant,
    waitingforentrytime: Instant,
    doneerly: Instant,
    headshard: usize,
    usurpingtime: Instant,
    is_validator: bool,
    is_staker: bool, // modify this depending on if staking??? is that already done?
    sent_onces: HashSet<Vec<u8>>,
    knownvalidators: HashMap<u64,NodeId>,
    announcevalidationtime: Instant,
    leaderip: Option<NodeId>,
    newest: u64,
    rmems: HashMap<u64,OTAccount>,
    rname: Vec<u8>,
    is_user: bool,
    moneyreset: Option<Vec<u8>>,
    sync_returnaddr: Option<NodeId>,
    sync_theirnum: u64,
    sync_lightning: char,
    outs: Option<Vec<(Account, Scalar)>>,
    groupsent: [bool;2],
    oldstk: Option<(Account, Vec<[u64;2]>, u64)>,
    cumtime: f64,
    blocktime: f64,
}
impl StakerNode {
    fn save(&self) {
        if !self.moneyreset.is_some() && !self.oldstk.is_some() {
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
                rmems: self.rmems.clone(),
                rname: self.rname.clone(),
                is_user: self.is_user,
                moneyreset: self.moneyreset.clone(),
                oldstk: self.oldstk.clone(),
                cumtime: self.cumtime,
                blocktime: self.blocktime,
            }; // just redo initial conditions on the rest
            let mut sn = bincode::serialize(&sn).unwrap();
            let mut f = File::create("myNode").unwrap();
            f.write_all(&mut sn).unwrap();
        }
    }
    fn load(inner: Node<Vec<u8>>, outer: Node<Vec<u8>>, gui_sender: channel::Sender<Vec<u8>>, gui_reciever: mpsc::Receiver<Vec<u8>>) -> StakerNode {
        let mut buf = Vec::<u8>::new();
        let mut f = File::open("myNode").unwrap();
        f.read_to_end(&mut buf).unwrap();

        let sn = bincode::deserialize::<SavedNode>(&buf).unwrap();
        let mut inner = inner;
        inner.dm(vec![], &sn.view.iter().map(|&x| NodeId::new(x, LocalNodeId::new(0))).collect::<Vec<_>>(), true);
        StakerNode {
            inner: inner,
            outer: outer,
            gui_sender,
            gui_reciever,
            timekeeper: Instant::now(),
            waitingforentrybool: true,
            waitingforleaderbool: false,
            waitingforleadertime: Instant::now(),
            waitingforentrytime: Instant::now(),
            usurpingtime: Instant::now(),
            txses: vec![], // if someone is not a leader for a really long time they'll have a wrongly long list of tx
            sigs: vec![],
            bannedlist: HashSet::new(),
            points: HashMap::new(),
            scalars: HashMap::new(),
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
            is_validator: false,
            is_staker: true,
            sent_onces: HashSet::new(), // maybe occasionally clear this or replace with vecdeq?
            knownvalidators: HashMap::new(),
            announcevalidationtime: Instant::now() - Duration::from_secs(10),
            doneerly: Instant::now(),
            leaderip: None,
            newest: 0u64,
            rmems: HashMap::new(),
            rname: vec![],
            is_user: sn.is_user,
            moneyreset: sn.moneyreset,
            sync_returnaddr: None,
            sync_theirnum: 0u64,
            sync_lightning: 'b',
            outs: None,
            groupsent: [false;2],
            oldstk: sn.oldstk,
            cumtime: sn.cumtime,
            blocktime: sn.blocktime,
        }
    }
    fn readblock(&mut self, lastblock: NextBlock, m: Vec<u8>) -> bool {
        let lastlightning = lastblock.tolightning();
        let l = bincode::serialize(&lastlightning).unwrap();
        self.readlightning(lastlightning,l,Some(m.clone()))
    }
    fn readlightning(&mut self, lastlightning: LightningSyncBlock, m: Vec<u8>, largeblock: Option<Vec<u8>>) -> bool {
        if lastlightning.bnum >= self.bnum {
            let com = self.comittee.par_iter().map(|x| x.par_iter().map(|y| *y as u64).collect::<Vec<_>>()).collect::<Vec<_>>();
            println!("someone's sending block {} with name: {:?}",lastlightning.bnum,lastlightning.last_name);
            println!("names match up: {}",lastlightning.last_name == self.lastname);
            if lastlightning.last_name != self.lastname {
                println!("{:?}\n{:?}",self.lastname,lastlightning.last_name);
            }
            println!("stkinfo: {:?}",self.stkinfo);
            if lastlightning.shards.len() == 0 {
                println!("Error in block verification: there is no shard");
                return false;
            }
            let v: bool;
            if (lastlightning.shards[0] as usize >= self.headshard) && (lastlightning.last_name == self.lastname) {
                if self.is_validator {
                    v = lastlightning.verify_multithread(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok();
                } else {
                    v = lastlightning.verify(&com[lastlightning.shards[0] as usize], &self.stkinfo).is_ok();
                }
            } else {
                v = false;
            }
            if v  {
                self.save();

                println!("smine: {:?}",self.smine);
                println!("all outer push pears: {:?}",self.outer.plumtree_node().all_push_peers());
                self.headshard = lastlightning.shards[0] as usize;

                println!("=========================================================\nyay!");
                // println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());

                self.overthrown.remove(&self.stkinfo[lastlightning.leader.pk as usize].0);
                println!("{{{{{{}}}}}}}}");
                println!("stkouts: {:?}",lastlightning.info.stkout);
                if self.stkinfo[lastlightning.leader.pk as usize].0 != self.leader {
                    self.overthrown.insert(self.leader);
                }
                println!("{{{{{{}}}}}}}}");

                for _ in self.bnum..lastlightning.bnum { // add whole different scannings for empty blocks
                    println!("I missed a block!");
                    let reward = reward(self.cumtime,self.blocktime);
                    self.cumtime += self.blocktime;
                    self.blocktime = blocktime(self.cumtime);

                    NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward);
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);


                    if let Some(oldstk) = &mut self.oldstk {
                        NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut oldstk.1, reward);
                    }




                    self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                    for i in 0..self.comittee.len() {
                        select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                    }
                    self.bnum += 1;
                }



                let reward = reward(self.cumtime,self.blocktime);
                // println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                if !(lastlightning.info.txout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.stkout.is_empty()) || (self.bnum - self.lastbnum > BLANKS_IN_A_ROW) {
                    let oldlocs = self.smine.iter().map(|x| x[0]).collect::<Vec<_>>();
                    let mut guitruster = !lastlightning.scanstk(&self.me, &mut self.smine, &mut self.sheight, &self.comittee, reward, &self.stkinfo);
                    guitruster = !lastlightning.scan(&self.me, &mut self.mine, &mut self.height, &mut self.alltagsever) && guitruster;
                    self.gui_sender.send(vec![guitruster as u8,1]).expect("there's a problem communicating to the gui!");

                    if !self.is_user {
                        lastlightning.update_bloom(&mut self.bloom,&self.is_validator);
                    }
                    if let Some(mut lastblock) = largeblock {
                        if !self.is_user {
                            println!("saving block...");
                            let mut f = File::create(format!("blocks/b{}",lastlightning.bnum)).unwrap();
                            f.write_all(&lastblock).unwrap(); // writing doesnt show up in blocks in vs code immediatly
                            let mut f = File::create(format!("blocks/l{}",lastlightning.bnum)).unwrap();
                            f.write_all(&m).unwrap(); // writing doesnt show up in blocks in vs code immediatly

                        }

                        if self.keylocation.contains(&(self.comittee[self.headshard][self.exitqueue[self.headshard][0]] as u64)) || self.keylocation.contains(&(self.comittee[self.headshard][self.exitqueue[self.headshard][1]] as u64)) || oldlocs != self.smine.iter().map(|x| x[0]).collect::<Vec<_>>() {
                            lastblock.push(3);
                            println!("-----------------------------------------------\nsending out the new block {}!\n-----------------------------------------------",lastlightning.bnum);
                            self.outer.broadcast_now(lastblock); /* broadcast the block to the outside world */
                        }
                    }
                    // as a user you dont save the file
                    self.keylocation = self.smine.iter().map(|x| x[0]).collect();
                    lastlightning.scan_as_noone(&mut self.stkinfo, &mut self.queue, &mut self.exitqueue, &mut self.comittee, reward, self.save_history);

                    self.lastbnum = self.bnum;
                    let mut hasher = Sha3_512::new();
                    hasher.update(m);
                    self.lastname = Scalar::from_hash(hasher).as_bytes().to_vec();
                } else {
                    self.gui_sender.send(vec![!NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut self.smine, reward) as u8,1]).expect("there's a problem communicating to the gui!");
                    NextBlock::pay_all_empty(&self.headshard, &mut self.comittee, &mut self.stkinfo, reward);
                }
                // println!("vecdeque lengths: {}, {}, {}",self.randomstakers.len(),self.queue[0].len(),self.exitqueue[0].len());
                self.votes[self.exitqueue[self.headshard][0]] = 0; self.votes[self.exitqueue[self.headshard][1]] = 0;
                self.newest = self.queue[self.headshard][0] as u64;
                for i in 0..self.comittee.len() {
                    select_stakers(&self.lastname,&self.bnum, &(i as u128), &mut self.queue[i], &mut self.exitqueue[i], &mut self.comittee[i], &self.stkinfo);
                }
                self.bnum += 1;

                if lastlightning.emptyness.is_some() {
                    self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z - lastlightning.emptyness.clone().unwrap().pk.iter().filter(|&&y| self.comittee[self.headshard][y as usize] == x).count() as i32).collect::<Vec<_>>();
                } else {
                    self.votes = self.votes.iter().zip(self.comittee[self.headshard].iter()).map(|(z,&x)| z + lastlightning.validators.clone().unwrap().iter().filter(|y| y.pk == x as u64).count() as i32).collect::<Vec<_>>();
                }


                
                println!("exitqueue: {:?}",self.exitqueue[self.headshard]);

                
                /* LEADER CHOSEN BY VOTES */
                let abouttoleave = self.exitqueue[self.headshard].range(..10).into_iter().map(|z| self.comittee[self.headshard][*z].clone()).collect::<HashSet<_>>();
                self.leader = self.stkinfo[*self.comittee[self.headshard].iter().zip(self.votes.iter()).max_by_key(|(x,&y)| {
                    if abouttoleave.contains(x) || self.overthrown.contains(&self.stkinfo[**x].0) {
                        i32::MIN
                    } else {
                        y
                    }
                }).unwrap().0].0;
                self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                    if lastlightning.info.stkout.contains(&location) {
                        None
                    } else {
                        let location = location - lastlightning.info.stkout.iter().map(|x| (*x < location) as u64).sum::<u64>();
                        Some((location,node))
                    }
                }).collect::<HashMap<_,_>>();
                if self.keylocation.iter().all(|&key| !self.queue[self.headshard].contains(&(key as usize))) && self.keylocation.iter().all(|&key| !self.comittee[self.headshard].contains(&(key as usize))) {
                    self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                        if self.queue[self.headshard].contains(&(location as usize)) {
                            Some((location,node.with_id(0)))
                        } else {
                            None
                        }
                    }).collect::<HashMap<_,_>>();
                } else {
                    self.knownvalidators = self.knownvalidators.iter().filter_map(|(&location,&node)| {
                        if self.queue[self.headshard].contains(&(location as usize)) || self.comittee[self.headshard].contains(&(location as usize)) {
                            Some((location,node.with_id(1)))
                        } else {
                            None
                        }
                    }).collect::<HashMap<_,_>>();
                }

                if let Some(&x) = self.knownvalidators.iter().filter(|&x| self.stkinfo[*x.0 as usize].0 == self.leader).map(|(_,&x)| x).collect::<Vec<_>>().get(0) {
                    self.leaderip = Some(x.with_id(1u64));
                } else {
                    self.leaderip = None;
                }
                /* LEADER CHOSEN BY VOTES */
                let mut mymoney = self.mine.iter().map(|x| self.me.receive_ot(&x.1).unwrap().com.amount.unwrap()).sum::<Scalar>().as_bytes()[..8].to_vec();
                mymoney.extend(self.smine.iter().map(|x| x[1]).sum::<u64>().to_le_bytes());
                mymoney.push(0);
                println!("my money:\n---------------------------------\n{:?}",mymoney);
                self.gui_sender.send(mymoney).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
                let mut thisbnum = self.bnum.to_le_bytes().to_vec();
                thisbnum.push(2);
                self.gui_sender.send(thisbnum).expect("something's wrong with the communication to the gui"); // this is how you send info to the gui
                println!("block {} name: {:?}",self.bnum, self.lastname);

                if self.bnum % 128 == 0 {
                    self.overthrown = HashSet::new();
                }
                let s = self.stkinfo.borrow();
                let bloom = self.bloom.borrow();
                println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\nhad {} tx",self.txses.len());
                self.txses.retain(|x| {
                    if let Ok(x) = bincode::deserialize::<PolynomialTransaction>(x) {
                        if x.inputs.last() == Some(&1) {
                            x.verifystk(s).is_ok()
                        } else {
                            x.tags.iter().all(|x| !bloom.contains(x.as_bytes())) && x.verify().is_ok()
                        }
                    } else {
                        false
                    }
                });
                println!("have {} tx",self.txses.len());
                

                self.send_panic_or_stop(&lastlightning, reward);

                if self.is_validator && self.inner.plumtree_node().all_push_peers().is_empty() {
                    for n in self.knownvalidators.iter() {
                        self.inner.join(n.1.with_id(1));
                    }
                }


                self.cumtime += self.blocktime;
                self.blocktime = blocktime(self.cumtime);

                self.is_user = self.smine.is_empty();
                self.sigs = vec![];
                self.groupsent = [false;2];
                self.points = HashMap::new();
                self.scalars = HashMap::new();
                self.doneerly = self.timekeeper;
                self.waitingforentrybool = true;
                self.waitingforleaderbool = false;
                self.waitingforleadertime = Instant::now();
                self.waitingforentrytime = Instant::now();
                self.timekeeper = Instant::now();
                self.usurpingtime = Instant::now();
                println!("block reading process done!!!");

                return true
            }
        }
        false
    }
    fn send_panic_or_stop(&mut self, lastlightning: &LightningSyncBlock, reward: f64) {
        if self.moneyreset.is_some() || self.oldstk.is_some() {
            if self.mine.len() < (self.moneyreset.is_some() as usize + self.oldstk.is_some() as usize) {
                let mut oldstkcheck = false;
                if let Some(oldstk) = &mut self.oldstk {
                    if !self.mine.iter().all(|x| x.1.com.amount.unwrap() != Scalar::from(oldstk.2)) {
                        oldstkcheck = true;
                    }
                    if !(lastlightning.info.stkout.is_empty() && lastlightning.info.stkin.is_empty() && lastlightning.info.txout.is_empty()) || (self.bnum - self.lastbnum > BLANKS_IN_A_ROW) {
                        lastlightning.scanstk(&oldstk.0, &mut oldstk.1, &mut self.sheight.clone(), &self.comittee, reward, &self.stkinfo);
                    } else {
                        NextBlock::pay_self_empty(&self.headshard, &self.comittee, &mut oldstk.1, reward);
                    }
                    oldstk.2 = oldstk.1.iter().map(|x| x[1]).sum::<u64>(); // maybe add a fee here?
                    let (loc, amnt): (Vec<u64>,Vec<u64>) = oldstk.1.iter().map(|x|(x[0],x[1])).unzip();
                    let inps = amnt.into_iter().map(|x| oldstk.0.receive_ot(&oldstk.0.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                    let mut outs = vec![];
                    let y = oldstk.2/2u64.pow(BETA as u32) + 1;
                    for _ in 0..y {
                        let stkamnt = Scalar::from(oldstk.2/y);
                        outs.push((&self.me,stkamnt));
                    }
                    let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(x.0,&x.1)).collect());
                    println!("about to verify!");
                    tx.verify().unwrap();
                    println!("finished to verify!");
                    let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                    loc.push(1);
                    let tx = tx.polyform(&loc); // push 0
                    if tx.verifystk(&self.stkinfo).is_ok() {
                        let mut txbin = bincode::serialize(&tx).unwrap();
                        self.txses.push(txbin.clone());
                        txbin.push(0);
                        self.outer.broadcast_now(txbin.clone());
                        self.inner.broadcast_now(txbin.clone());
                    }
                }
                if oldstkcheck {
                    self.oldstk = None;
                }
                if self.mine.len() > 0 && self.oldstk.is_some() {
                    self.moneyreset = None;
                }
                if let Some(x) = self.moneyreset.clone() {
                    self.outer.broadcast(x);
                }
            } else {
                self.moneyreset = None;
            }
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
            // print!(".");

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
            if self.keylocation.iter().all(|keylocation| !self.comittee[self.headshard].contains(&(*keylocation as usize)) ) { // if you're not in the comittee
                self.is_staker = true;
                self.is_validator = false;
                if (self.doneerly.elapsed().as_secs() > self.blocktime as u64) && (self.doneerly.elapsed().as_secs() > self.timekeeper.elapsed().as_secs()) {
                    self.waitingforentrybool = true;
                    self.waitingforleaderbool = false;
                    self.waitingforleadertime = Instant::now();
                    self.waitingforentrytime = Instant::now();
                    self.timekeeper = Instant::now();
                    self.doneerly = Instant::now();
                    self.usurpingtime = Instant::now();

                    if self.keylocation.contains(&(self.newest as u64)) {
                        let m = bincode::serialize(&self.txses).unwrap();
                        println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
                        let mut m = Signature::sign_message_nonced(&self.key, &m, &self.newest,&self.bnum);
                        m.push(1u8);
                        self.inner.broadcast(m);
                    }
                }
                if self.doneerly.elapsed() > self.timekeeper.elapsed() {
                    self.waitingforleadertime = Instant::now();
                    self.waitingforentrytime = Instant::now();
                    self.timekeeper = Instant::now();
                    self.usurpingtime = Instant::now();
                }
            } else { // if you're in the comittee
                self.is_staker = false;
                self.is_validator = true;
            }
            self.keylocation.clone().iter().for_each(|keylocation| { // get these numbers to be based on something
                let headqueue = self.queue[self.headshard].clone();


                if headqueue.range(REPLACERATE..WARNINGTIME).any(|&x| x as u64 != *keylocation) {
                    self.is_staker = true;
                    let message = bincode::serialize(self.outer.plumtree_node().id()).unwrap();
                    if self.sent_onces.insert(message.clone().into_iter().chain(self.bnum.to_le_bytes().to_vec().into_iter()).collect::<Vec<_>>()) {
                        println!("broadcasting name!");
                        let mut evidence = Signature::sign_message(&self.key, &message, keylocation);
                        evidence.push(118); // v
                        self.outer.broadcast(evidence); // add a dm your transactions to this list section (also add them to your list of known validators if they are validators)
                    }
                }
                if headqueue.range(0..REPLACERATE).any(|&x| x as u64 != *keylocation) {
                    self.is_staker = true;
                    self.is_validator = true;
                    if self.announcevalidationtime.elapsed().as_secs() > 10 { // every 10 seconds you say your name
                        let message = bincode::serialize(self.inner.plumtree_node().id()).unwrap();
                        let mut evidence = Signature::sign_message(&self.key, &message, &keylocation);
                        evidence.push(118); // v
                        self.inner.dm_now(evidence,&self.knownvalidators.iter().filter_map(|(&location,node)| {
                            let node = node.with_id(1);
                            if self.comittee[self.headshard].contains(&(location as usize)) && !(self.inner.plumtree_node().all_push_peers().contains(&node) | (node == self.inner.plumtree_node().id)) {
                                println!("(((((((((((((((((((((((((((((((((((((((((((((((dm'ing validators)))))))))))))))))))))))))))))))))))))))))))))))))))))");
                                Some(node)
                            } else {
                                None
                            }
                        }).collect::<Vec<_>>(), true);
                        self.announcevalidationtime = Instant::now();
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
                while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.inner.poll()) {
                    let msg = fullmsg.message.clone();
                    if !self.bannedlist.contains(&msg.id.node()) {
                        let mut m = msg.payload.to_vec();
                        if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a anyone except user
                            if mtype == 2 {print!("#{:?}", mtype);}
                            else {println!("# MESSAGE TYPE: {:?} FROM: {:?}", mtype,msg.id.node());}


                            if mtype == 1 {
                                if let Some(who) = Signature::recieve_signed_message_nonced(&mut m, &self.stkinfo, &self.bnum) {
                                    if (who == self.newest) || (self.stkinfo[who as usize].0 == self.leader) {
                                        if let Ok(m) = bincode::deserialize::<Vec<Vec<u8>>>(&m) {
                                            let m = m.into_par_iter().filter_map(|x|
                                                if let Ok(x) = bincode::deserialize(&x) {
                                                    Some(x)
                                                } else {
                                                    None
                                                }
                                            ).collect::<Vec<PolynomialTransaction>>();

                                            for keylocation in &self.keylocation {
                                                let m = NextBlock::valicreate(&self.key, &keylocation, &self.leader, &m, &(self.headshard as u16), &self.bnum, &self.lastname, &self.bloom, &self.stkinfo);
                                                if m.txs.len() > 0 || self.groupsent[0] {
                                                    println!("{:?}",m.txs.len());
                                                    let mut m = bincode::serialize(&m).unwrap();
                                                    m.push(2);
                                                    for _ in self.comittee[self.headshard].iter().filter(|&x|*x as u64 == *keylocation).collect::<Vec<_>>() {
                                                        if let Some(x) = self.leaderip {
                                                            self.inner.dm(m.clone(), vec![&x], false);
                                                        } else {
                                                            self.inner.broadcast(m.clone());
                                                        }
                                                    }
                                                } else if (m.txs.len() == 0) && (m.emptyness.is_none()){
                                                    println!("going for empty block");
                                                    if !self.groupsent[0] {
                                                        self.groupsent[0] = true;
                                                        let m = MultiSignature::gen_group_x(&self.key,&self.bnum).as_bytes().to_vec();// add ,(self.headshard as u16).to_le_bytes().to_vec() to m
                                                        let mut m = Signature::sign_message_nonced(&self.key, &m, keylocation, &self.bnum);
                                                        m.push(4u8);
                                                        if !self.comittee[self.headshard].iter().all(|&x|x as u64 != *keylocation) {
                                                            // self.inner.broadcast(m.clone());
                                                            if let Some(x) = self.leaderip {
                                                                println!("I'm dm'ing a MESSAGE TYPE 4 to {:?}",x);
                                                                self.inner.dm(m, vec![&x], false);
                                                            } else {
                                                                println!("I'm sending a MESSAGE TYPE 4 to {:?}",self.inner.plumtree_node().all_push_peers());
                                                                self.inner.broadcast(m);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            self.waitingforentrybool = false;
                                            self.waitingforleaderbool = true;
                                            self.waitingforleadertime = Instant::now();
                                            self.inner.handle_gossip_now(fullmsg, true);
                                        } else {
                                            self.inner.handle_gossip_now(fullmsg, false);
                                        }
                                    } else {
                                        self.inner.handle_gossip_now(fullmsg, false);
                                    }
                                }
                            } else if mtype == 2 {
                                if let Ok(sig) = bincode::deserialize(&m) {
                                    self.sigs.push(sig);
                                    self.inner.handle_gossip_now(fullmsg, true);
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 3 {
                                if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                    if self.readblock(lastblock, m) {
                                        self.inner.handle_gossip_now(fullmsg, true);
                                    } else {
                                        self.inner.handle_gossip_now(fullmsg, false);
                                    }
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 4 {
                                println!("recieving points phase: {}\nleader: {:?}",!self.points.contains_key(&usize::MAX),self.leader);
                                if !self.points.contains_key(&usize::MAX) {
                                    if let Some(pk) = Signature::recieve_signed_message_nonced(&mut m, &self.stkinfo, &self.bnum) {
                                        let pk = pk as usize;
                                        if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                            if let Ok(m) = m.try_into() {
                                                if let Some(m) = CompressedRistretto(m).decompress() {
                                                    println!("got sent point from {}",pk);
                                                    self.points.insert(pk,m);
                                                }
                                            }
                                            
                                        }
                                    }
                                }
                                self.inner.handle_gossip_now(fullmsg, true);
                            } else if mtype == 5 {
                                if let Ok(m) = m.try_into() {
                                    if self.groupsent[1] {
                                        self.inner.handle_gossip_now(fullmsg, true);
                                    } else {
                                        self.groupsent[1] = true;
                                        let xt = CompressedRistretto(m);
                                        let mut mess = self.leader.as_bytes().to_vec();
                                        mess.extend(&self.lastname);
                                        mess.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                                        println!("you're trying to send a scalar!");
                                        for keylocation in self.keylocation.iter() {
                                            let mut m = Signature::sign_message_nonced(&self.key, &MultiSignature::try_get_y(&self.key, &mess, &xt, &self.bnum).as_bytes().to_vec(), keylocation, &self.bnum);
                                            m.push(6u8);
                                            if !self.comittee[self.headshard].iter().all(|&x| !self.keylocation.contains(&(x as u64))) {
                                                if let Some(x) = self.leaderip {
                                                    self.inner.dm(m, vec![&x], true);
                                                } else {
                                                    self.inner.broadcast(m);
                                                }
                                            }
                                        }
                                        self.waitingforleadertime = Instant::now();
                                        self.inner.handle_gossip_now(fullmsg, true);
                                    }
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 6 {
                                println!("recieving scalars phase: {}",self.points.contains_key(&usize::MAX));
                                if let Some(pk) = Signature::recieve_signed_message_nonced(&mut m, &self.stkinfo, &self.bnum) {
                                    let pk = pk as usize;
                                    if !self.comittee[self.headshard].par_iter().all(|x| x!=&pk) {
                                        println!("got sent a scalar from {}",pk);
                                        if let Ok(m) = m.try_into() {
                                            self.scalars.insert(pk,Scalar::from_bits(m));
                                            self.inner.handle_gossip_now(fullmsg, true);
                                        } else {
                                            self.inner.handle_gossip_now(fullmsg, false);
                                        }
                                    } else {
                                        self.inner.handle_gossip_now(fullmsg, false);
                                    }
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else {
                                self.inner.handle_gossip_now(fullmsg, false);
                            }
                        }
                    }
                    did_something = true;
                }
                if (self.waitingforleadertime.elapsed().as_secs() > (0.5*self.blocktime) as u64) && self.waitingforleaderbool {
                    self.waitingforleadertime = Instant::now();
                    /* change the leader, also add something about only changing the leader if block is free */

                    self.overthrown.insert(self.leader);
                    self.leader = self.stkinfo[*self.comittee[0].iter().zip(self.votes.iter()).max_by_key(|(&x,&y)| { // i think it does make sense to not care about whose going to leave soon here
                        let candidate = self.stkinfo[x];
                        if self.overthrown.contains(&candidate.0) {
                            i32::MIN
                        } else {
                            y
                        }
                    }).unwrap().0].0;
                    if let Some(&x) = self.knownvalidators.iter().filter(|&x| self.stkinfo[*x.0 as usize].0 == self.leader).map(|(_,&x)| x).collect::<Vec<_>>().get(0) {
                        self.leaderip = Some(x.with_id(1u64));
                    } else {
                        self.leaderip = None;
                    }
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
                    if (self.sigs.len() > SIGNING_CUTOFF) && (self.timekeeper.elapsed().as_secs() > (0.25*self.blocktime) as u64) {
                        let lastblock = NextBlock::finish(&self.key, &self.keylocation.iter().next().unwrap(), &self.sigs, &self.comittee[self.headshard].par_iter().map(|x|*x as u64).collect::<Vec<u64>>(), &(self.headshard as u16), &self.bnum, &self.lastname, &self.stkinfo);
        
                        if lastblock.validators.is_some() {
                            lastblock.verify(&self.comittee[self.headshard].iter().map(|&x| x as u64).collect::<Vec<_>>(), &self.stkinfo).unwrap();

                            let mut m = bincode::serialize(&lastblock).unwrap();
                            m.push(3u8);
                            self.inner.broadcast(m);
        
        
                            self.sigs = vec![];
        
                            println!("made a block with {} transactions!",lastblock.txs.len());
                        } else {
                            println!("failed to make a block :(");

                            self.sigs = vec![];
                            let m = bincode::serialize(&self.txses).unwrap();
                            println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
                            let mut m = Signature::sign_message_nonced(&self.key, &m, &(self.comittee[self.headshard].clone().into_iter().filter(|&who| self.stkinfo[who].0 == self.leader).collect::<Vec<_>>()[0] as u64),&self.bnum);
                            m.push(1u8);
                            self.inner.broadcast(m);

                        }
                        did_something = true;
                    }
                    if self.points.get(&usize::MAX).is_none() && (self.timekeeper.elapsed().as_secs() > (0.25*self.blocktime) as u64) && (self.comittee[self.headshard].iter().filter(|&x| self.points.contains_key(x)).count() > SIGNING_CUTOFF) {
                        // should prob check that validators are accurate here?
                        let points = self.points.par_iter().map(|x| *x.1).collect::<Vec<_>>();
                        let mut m = MultiSignature::sum_group_x(&points).as_bytes().to_vec();
                        m.push(5u8);
                        self.inner.broadcast(m);
                        // self.points = HashMap::new();
                        self.points.insert(usize::MAX,MultiSignature::sum_group_x(&points).decompress().unwrap());
                        println!("scalar time!");
                        did_something = true;
        
                    }
                    if self.points.get(&usize::MAX).is_some() && (self.timekeeper.elapsed().as_secs() > (0.5*self.blocktime) as u64) {
                        let k = self.scalars.keys().collect::<HashSet<_>>();
                        if self.comittee[self.headshard].iter().filter(|x| k.contains(x)).count() > SIGNING_CUTOFF {
                            let sumpt = self.points.remove(&usize::MAX).unwrap();
            
                            let keys = self.points.clone();
                            let mut keys = keys.keys().collect::<Vec<_>>();
                            let mut m = self.leader.as_bytes().to_vec();
                            m.extend(&self.lastname);
                            m.extend(&(self.headshard as u16).to_le_bytes().to_vec());
                            let mut s = Sha3_512::new();
                            s.update(&m);
                            s.update(sumpt.compress().as_bytes());
                            let e = Scalar::from_hash(s);

                            // println!("keys: {:?}",keys);
                            let k = keys.len();
                            keys.retain(|&x|
                                if self.scalars.contains_key(&x) {
                                    (self.points[x] + e*self.stkinfo[*x].0.decompress().unwrap() == self.scalars[x]*PEDERSEN_H()) && self.comittee[self.headshard].contains(x)
                                } else {
                                    false
                                }
                            );
                            if k == keys.len() {
                                let failed_validators = self.comittee[self.headshard].iter().enumerate().filter_map(|(i,x)|
                                    if self.points.contains_key(x) {
                                        None
                                    } else {
                                        Some(i as u8)
                                    }
                                ).collect::<Vec<_>>();
                                // println!("e: {:?}",e);
                                // println!("y: {:?}",self.scalars.values().map(|x| *x).sum::<Scalar>());
                                // println!("x: {:?}",sumpt.compress());
                                // println!("not who: {:?}",failed_validators); // <-------this is fucked up somehow
                                // println!("comittee: {:?}",self.comittee[self.headshard]);
                                let mut lastblock = NextBlock::default();
                                lastblock.bnum = self.bnum;
                                lastblock.emptyness = Some(MultiSignature{x: sumpt.compress(), y: MultiSignature::sum_group_y(&self.scalars.values().map(|x| *x).collect::<Vec<_>>()), pk: failed_validators});
                                lastblock.last_name = self.lastname.clone();
                                lastblock.shards = vec![self.headshard as u16];
                
                                
                                let m = vec![(self.headshard as u16).to_le_bytes().to_vec(),self.bnum.to_le_bytes().to_vec(),self.lastname.clone(),bincode::serialize(&lastblock.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
                                let mut s = Sha3_512::new();
                                s.update(&m);
                                let leader = Signature::sign(&self.key, &mut s,&self.keylocation.iter().next().unwrap());
                                lastblock.leader = leader;

                                // println!("sum of points is accurate: {}",sumpt == self.points.iter().map(|x| x.1).sum());
                                if lastblock.verify(&self.comittee[self.headshard].iter().map(|x| *x as u64).collect(),&self.stkinfo).is_ok() {
                                    println!("block verified!");
                                    let mut m = bincode::serialize(&lastblock).unwrap();
                                    println!("sending off block {}!!!",lastblock.bnum);
                                    m.push(3u8);
                                    self.inner.broadcast(m);
                                } else {
                                    println!("block NOT verified... this shouldn't happen... restarting this selection stuff");
                                    let m = bincode::serialize(&self.txses).unwrap();
                                    println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
                                    let mut m = Signature::sign_message_nonced(&self.key, &m, &(self.comittee[self.headshard].clone().into_iter().filter(|&who| self.stkinfo[who].0 == self.leader).collect::<Vec<_>>()[0] as u64),&self.bnum); // add wipe last few histories button? (save 2 states, 1 tracking from before)
                                    m.push(1u8);
                                }
                                self.points = HashMap::new();
                                self.scalars = HashMap::new();
                                self.sigs = vec![];
                                
                            } else { // failed to make a small signature block
                                let m = bincode::serialize(&self.txses).unwrap();
                                println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
                                let mut m = Signature::sign_message_nonced(&self.key, &m, &(self.comittee[self.headshard].clone().into_iter().filter(|&who| self.stkinfo[who].0 == self.leader).collect::<Vec<_>>()[0] as u64),&self.bnum); // add wipe last few histories button? (save 2 states, 1 tracking from before)
                                m.push(1u8);
                                println!("a validator was corrupted I'm restarting this bitch");
                                self.inner.broadcast(m);
                                self.timekeeper = Instant::now();
                            }
                        } else {
                            let m = bincode::serialize(&self.txses).unwrap();
                            println!("_._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._._.\nsending {} txses!",self.txses.len());
                            let mut m = Signature::sign_message_nonced(&self.key, &m, &(self.comittee[self.headshard].clone().into_iter().filter(|&who| self.stkinfo[who].0 == self.leader).collect::<Vec<_>>()[0] as u64),&self.bnum);
                            m.push(1u8);
                            self.inner.broadcast(m);
                            self.timekeeper = Instant::now();
                        }
                        did_something = true;
                    }
                }
                // }
                if self.waitingforentrybool && (self.waitingforentrytime.elapsed().as_secs() > (0.5*self.blocktime) as u64) {
                    self.waitingforentrybool = false;
                    for keylocation in &self.keylocation {
                        if !self.groupsent[0] {
                            self.groupsent[0] = true;
                            let m = MultiSignature::gen_group_x(&self.key, &self.bnum).as_bytes().to_vec();// add ,(self.headshard as u16).to_le_bytes().to_vec() to m
                            let mut m = Signature::sign_message_nonced(&self.key, &m, keylocation, &self.bnum);
                            m.push(4u8);
                            if self.comittee[self.headshard].contains(&(*keylocation as usize)) {
                                println!("I'm sending a MESSAGE TYPE 4 to {:?}",self.inner.plumtree_node().all_push_peers());
                                self.inner.broadcast(m);
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
            if self.is_staker { // users have is_staker true
                if let Some(addr) = self.sync_returnaddr {
                    for b in self.sync_theirnum..std::cmp::min(self.sync_theirnum+10, self.bnum) {
                        let file = format!("blocks/{}{}",self.sync_lightning,b);
                        println!("checking for file {:?}...",file);
                        if let Ok(mut file) = File::open(file) {
                            let mut x = vec![];
                            file.read_to_end(&mut x).unwrap();
                            println!("sending block {} of {}",b,self.bnum);
                            if self.sync_lightning == 'l' {
                                x.push(108); //l
                            } else {
                                x.push(3);
                            }
                            self.outer.dm(x,&vec![addr],false);
                        }
                        self.sync_theirnum += 1;
                    }
                }



                while let Async::Ready(Some(fullmsg)) = track_try_unwrap!(self.outer.poll()) {
                    let msg = fullmsg.message.clone();
                    if !self.bannedlist.contains(&msg.id.node()) {
                        let mut m = msg.payload.to_vec();
                        if let Some(mtype) = m.pop() { // dont do unwraps that could mess up a anyone except user
                            println!("# MESSAGE TYPE: {:?} FROM: {:?}", mtype,msg.id.node());


                            if mtype == 0 {
                                let m = m[..std::cmp::min(m.len(),10_000)].to_vec();
                                if let Ok(t) = bincode::deserialize::<PolynomialTransaction>(&m) {
                                    if !self.is_user {
                                        let ok = {
                                            if t.inputs.last() == Some(&1) {
                                                t.verifystk(&self.stkinfo).is_ok()
                                            } else {
                                                t.verify().is_ok()
                                            }
                                        };
                                        let bloom = self.bloom.borrow();
                                        if t.tags.par_iter().all(|y| !bloom.contains(y.as_bytes())) && ok {
                                            self.txses.push(m);
                                            print!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\ngot a tx, now at {}!",self.txses.len());
                                            self.outer.handle_gossip_now(fullmsg, true);
                                        }
                                    } else {
                                        self.outer.handle_gossip_now(fullmsg, true);
                                    }
                                } else {
                                    self.outer.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 3 {
                                if let Ok(lastblock) = bincode::deserialize::<NextBlock>(&m) {
                                    let s = self.readblock(lastblock, m);
                                    self.outer.handle_gossip_now(fullmsg, s);
                                } else {
                                    self.outer.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 60 /* < */ { // redo sync request
                                let mut mynum = self.bnum.to_le_bytes().to_vec();
                                if self.is_user {
                                    mynum.push(108); //l
                                } else {
                                    mynum.push(102); //f
                                }
                                mynum.push(121);
                                let mut friend = self.outer.plumtree_node().all_push_peers();
                                friend.remove(&msg.id.node());
                                friend.remove(self.outer.plumtree_node().id());
                                let friend = friend.into_iter().collect::<Vec<_>>();
                                if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                                    println!("asking for help from {:?}",friend);
                                    self.outer.dm(mynum, &[*friend], false);
                                } else {
                                    println!("you're isolated");
                                }
                            } else if mtype == 108 /* l */ {
                                if let Ok(lastblock) = bincode::deserialize::<LightningSyncBlock>(&m) {
                                    self.readlightning(lastblock, m, None); // that whole thing with 3 and 8 makes it super unlikely to get more blocks (expecially for my small thing?)
                                }
                            } else if mtype == 113 /* q */ { // they just sent you a ring member
                                self.rmems.insert(u64::from_le_bytes(m[64..72].try_into().unwrap()),History::read_raw(&m));
                            } else if mtype == 114 /* r */ { // answer their ring question
                                let mut y = m[..8].to_vec();
                                let mut x = History::get_raw(&u64::from_le_bytes(y.clone().try_into().unwrap())).to_vec();
                                x.append(&mut y);
                                x.push(113);
                                self.outer.dm(x,&vec![msg.id.node()],false);
                            } else if mtype == 118 /* v */ {
                                if let Some(who) = Signature::recieve_signed_message(&mut m, &self.stkinfo) {
                                    let m = bincode::deserialize::<NodeId>(&m).unwrap();
                                    if self.queue[self.headshard].contains(&(who as usize)) {
                                        self.knownvalidators.insert(who,m.with_id(0));
                                    }
                                    self.outer.handle_gossip_now(fullmsg, true);
                                } else {
                                    self.inner.handle_gossip_now(fullmsg, false);
                                }
                            } else if mtype == 121 /* y */ {
                                if !self.is_user {
                                    if self.sync_returnaddr.is_none() {
                                        if let Some(theyfast) = m.pop() {
                                            if let Ok(m) = m.try_into() {
                                                self.sync_returnaddr = Some(msg.id.node());
                                                self.sync_theirnum = u64::from_le_bytes(m);
                                                if theyfast == 108 {
                                                    self.sync_lightning = 'l';
                                                } else {
                                                    self.sync_lightning = 'b';
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    if msg.id.node() != *self.outer.plumtree_node().id() {
                                        self.outer.dm(vec![60], &[msg.id.node()], false);
                                    }
                                }
                            } else {
                                self.inner.handle_gossip_now(fullmsg, false);
                            }
                        }
                    }
                    did_something = true;
                }
            }













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
            if self.is_user {
                if let Some(outs) = self.outs.clone() {
                    let ring = recieve_ring(&self.rname).expect("shouldn't fail");
                    let mut rlring = ring.iter().map(|x| self.rmems[x].clone()).collect::<Vec<OTAccount>>();
                    rlring.iter_mut().for_each(|x|if let Ok(y)=self.me.receive_ot(&x) {*x = y;});
                    let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                    if tx.verify().is_ok() {
                        let tx = tx.polyform(&self.rname);
                        // tx.verify().unwrap(); // as a user you won't be able to check this
                        let mut txbin = bincode::serialize(&tx).unwrap();
                        txbin.push(0);
                        let needtosend = (txbin,self.mine.iter().map(|x| *x.0).collect::<Vec<_>>());
                        // self.needtosend = Some(needtosend.clone());
                        if self.knownvalidators.len() > 0 {
                            self.outer.dm_now(needtosend.0.clone(),self.knownvalidators.iter().map(|x| x.1).collect::<Vec<_>>(),false);
                        } else {
                            self.outer.broadcast_now(needtosend.0.clone());
                        }
                        println!("transaction made!");
                        self.outs = None;
                    } else {
                        println!("you can't make that transaction, user!");
                    }
                }
            }
            while let Async::Ready(Some(mut m)) = self.gui_reciever.poll().expect("Never fails") {
                println!("got message from gui!\n{}",String::from_utf8_lossy(&m));
                if let Some(istx) = m.pop() {
                    if istx == 33 /* ! */ {
                        let txtype = m.pop().unwrap();
                        let mut outs = vec![];
                        while m.len() > 0 {
                            let mut pks = vec![];
                            for _ in 0..3 { // read the pk address
                                let h1 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)).collect::<Vec<_>>();
                                let h2 = m.drain(..32).collect::<Vec<_>>().iter().map(|x| (x-97)*16).collect::<Vec<_>>();
                                pks.push(CompressedRistretto(h1.into_iter().zip(h2).map(|(x,y)|x+y).collect::<Vec<u8>>().try_into().unwrap()));
                            }
                            let x: [u8;8] = m.drain(..8).collect::<Vec<_>>().try_into().unwrap();
                            // println!("hi {:?}",x);
                            let x = u64::from_le_bytes(x);
                            println!("amounts {:?}",x);
                            // println!("ha {:?}",1u64.to_le_bytes());
                            let y = x/2u64.pow(BETA as u32) + 1;
                            println!("need to split this up into {} txses!",y);
                            let recv = Account::from_pks(&pks[0], &pks[1], &pks[2]);
                            for _ in 0..y {
                                let amnt = Scalar::from(x/y);
                                outs.push((recv,amnt));
                            }
                        }

                        let mut txbin: Vec<u8>;
                        if txtype == 33 /* ! */ {
                            let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            if self.is_user {
                                if self.mine.len() > 0 {
                                    let helpers = self.outer.plumtree_node().all_push_peers().into_iter().collect::<Vec<_>>();
                                
                                    let (loc, acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(*x.0,x.1.clone())).unzip();
                
                                    println!("loc: {:?}",loc);
                                    println!("height: {}",self.height); // need to get the true height first!
                                    for (i,j) in loc.iter().zip(acc) {
                                        println!("i: {}, j.pk: {:?}",i,j.pk.compress());
                                        self.rmems.insert(*i,j);
                                    }
                                    
                                    // maybe have bigger rings than 5? it's a choice i dont forbid anything
                                    self.rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + 4), &self.height);
                                    let ring = recieve_ring(&self.rname).expect("shouldn't fail");
                                    let ring = ring.into_iter().filter(|x| loc.iter().all(|y|x!=y)).collect::<Vec<_>>();
                                    println!("ring:----------------------------------\n{:?}",ring);
                                    let alen = helpers.len();
                                    for (i,r) in ring.iter().enumerate() {
                                        let mut r = r.to_le_bytes().to_vec();
                                        r.push(114u8);
                                        self.outer.dm(r,&[helpers[i%alen],helpers[(i+1)%alen]],false); // worry about txting self
                                    }

                                    self.outs = Some(outs);
                                }
                                txbin = vec![];
                            } else {
                                let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16 + 4), &self.height);
                                let ring = recieve_ring(&rname).expect("shouldn't fail");
                                println!("ring: {:?}",ring);
                                println!("mine: {:?}",acc.iter().map(|x|x.pk.compress()).collect::<Vec<_>>());
                                // println!("ring: {:?}",ring.iter().map(|x|OTAccount::summon_ota(&History::get(&x)).pk.compress()).collect::<Vec<_>>());
                                let mut rlring = ring.into_iter().map(|x| {
                                    let x = OTAccount::summon_ota(&History::get(&x));
                                    if acc.iter().all(|a| a.pk != x.pk) {
                                        println!("not mine!");
                                        x
                                    } else {
                                        println!("mine!");
                                        acc.iter().filter(|a| a.pk == x.pk).collect::<Vec<_>>()[0].to_owned()
                                    }
                                }).collect::<Vec<OTAccount>>();
                                println!("ring len: {:?}",rlring.len());
                                let me = self.me;
                                rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                                let tx = Transaction::spend_ring(&rlring, &outs.par_iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                                let tx = tx.polyform(&rname);
                                if tx.verify().is_ok() {
                                    txbin = bincode::serialize(&tx).unwrap();
                                    println!("transaction made!");
                                    // self.needtosend = Some((txbin.iter().chain(vec![&0u8]).map(|x| *x).collect(),self.mine.iter().map(|x| *x.0).collect::<Vec<_>>()));
                                } else {
                                    txbin = vec![];
                                    println!("you can't make that transaction!");
                                }
                            }
                        } else if txtype == 63 /* ? */ {
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.iter().map(|x|(x[0] as u64,x[1].clone())).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();
                            let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x|(&x.0,&x.1)).collect::<Vec<(&Account,&Scalar)>>());
                            println!("about to verify!");
                            tx.verify().unwrap();
                            println!("finished to verify!");
                            let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                txbin = bincode::serialize(&tx).unwrap();
                                println!("sending tx!");
                            } else {
                                txbin = vec![];
                                println!("you can't make that transaction!");
                            }
                        } else {
                            txbin = vec![];
                            println!("somethings wrong with your query!");

                        }
                        if !txbin.is_empty() {
                            self.txses.push(txbin.clone());
                            txbin.push(0);
                            self.outer.dm_now(txbin.clone(),self.knownvalidators.iter().map(|x| x.1).collect::<Vec<_>>(),false);
                            self.outer.broadcast_now(txbin);
                        }
                    } else if istx == u8::MAX /* panic button */ {
                        
                        let amnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        // let amnt = Scalar::from(amnt);
                        let mut stkamnt = u64::from_le_bytes(m.drain(..8).collect::<Vec<_>>().try_into().unwrap());
                        // let mut stkamnt = Scalar::from(stkamnt);
                        if stkamnt == amnt {
                            stkamnt -= 1;
                        }
                        let newacc = Account::new(&format!("{}",String::from_utf8_lossy(&m)));
                        println!("understood command");
                        if self.mine.len() > 0 {
                            let (loc, _acc): (Vec<u64>,Vec<OTAccount>) = self.mine.iter().map(|x|(x.0,x.1.clone())).unzip();

                            println!("remembered owned accounts");
                            let rname = generate_ring(&loc.iter().map(|x|*x as usize).collect::<Vec<_>>(), &(loc.len() as u16), &self.height);
                            let ring = recieve_ring(&rname).expect("shouldn't fail");

                            println!("made rings");
                            /* this is where people send you the ring members */ 
                            // let mut rlring = ring.into_par_iter().map(|x| OTAccount::summon_ota(&History::get(&x))).collect::<Vec<OTAccount>>();
                            let mut rlring = ring.iter().map(|&x| self.mine.iter().filter(|(&y,_)| y == x).collect::<Vec<_>>()[0].1.clone()).collect::<Vec<OTAccount>>();
                            /* this is where people send you the ring members */ 
                            let me = self.me;
                            rlring.iter_mut().for_each(|x|if let Ok(y)=me.receive_ot(&x) {*x = y;});
                            
                            let mut outs = vec![];
                            let y = amnt/2u64.pow(BETA as u32) + 1;
                            for _ in 0..y {
                                let amnt = Scalar::from(amnt/y);
                                outs.push((&newacc,amnt));
                            }
                            let tx = Transaction::spend_ring(&rlring, &outs.iter().map(|x| (x.0,&x.1)).collect());

                            println!("{:?}",rlring.iter().map(|x| x.com.amount).collect::<Vec<_>>());
                            println!("{:?}",amnt);
                            if tx.verify().is_ok() {
                                let tx = tx.polyform(&rname);
                                // tx.verify().unwrap(); // as a user you won't be able to check this
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.push(txbin.clone());
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                self.moneyreset = Some(txbin);
                                println!("transaction made!");
                            } else {
                                println!("you can't make that transaction, user!");
                            }
                        }


                        if self.smine.len() > 0 {
                            let (loc, amnt): (Vec<u64>,Vec<u64>) = self.smine.iter().map(|x|(x[0],x[1])).unzip();
                            let inps = amnt.into_iter().map(|x| self.me.receive_ot(&self.me.derive_stk_ot(&Scalar::from(x))).unwrap()).collect::<Vec<_>>();


                            let mut outs = vec![];
                            let y = stkamnt/2u64.pow(BETA as u32) + 1;
                            for _ in 0..y {
                                let stkamnt = Scalar::from(stkamnt/y);
                                outs.push((&newacc,stkamnt));
                            }
                            let tx = Transaction::spend_ring(&inps, &outs.iter().map(|x| (x.0,&x.1)).collect());
                            println!("about to verify!");
                            tx.verify().unwrap();
                            println!("finished to verify!");
                            let mut loc = loc.into_iter().map(|x| x.to_le_bytes().to_vec()).flatten().collect::<Vec<_>>();
                            loc.push(1);
                            let tx = tx.polyform(&loc); // push 0
                            if tx.verifystk(&self.stkinfo).is_ok() {
                                let mut txbin = bincode::serialize(&tx).unwrap();
                                self.txses.push(txbin.clone());
                                txbin.push(0);
                                self.outer.broadcast_now(txbin.clone());
                                self.oldstk = Some((self.me.clone(),self.smine.clone(),stkamnt));
                                println!("sending tx!");
                            } else {
                                println!("you can't make that transaction!");
                            }
                        }


                        self.mine = HashMap::new();
                        self.smine = vec![];
                        self.me = newacc;
                        self.key = self.me.stake_acc().receive_ot(&self.me.stake_acc().derive_stk_ot(&Scalar::from(1u8))).unwrap().sk.unwrap();
                        self.keylocation = HashSet::new();
                        let mut m1 = self.me.name().as_bytes().to_vec();
                        m1.extend([0,u8::MAX]);
                        let mut m2 = self.me.stake_acc().name().as_bytes().to_vec();
                        m2.extend([1,u8::MAX]);
                        self.gui_sender.send(m1).expect("should be working");
                        self.gui_sender.send(m2).expect("should be working");

                    } else if istx == 121 /* y */ {
                        let mut mynum = self.bnum.to_le_bytes().to_vec();
                        if self.is_user && self.oldstk.is_none(){
                            mynum.push(108); //l
                        } else {
                            mynum.push(102); //f
                        }
                        mynum.push(121);
                        let mut friend = self.outer.plumtree_node().all_push_peers();
                        friend.remove(self.outer.plumtree_node().id());
                        let friend = friend.into_iter().collect::<Vec<_>>();
                        if let Some(friend) = friend.choose(&mut rand::thread_rng()) {
                            println!("asking for help from {:?}",friend);
                            self.outer.dm(mynum, &[*friend], false);
                        } else {
                            println!("you're isolated");
                        }
                    } else if istx == 42 /* * */ { // ips to talk to
                        let m = String::from_utf8_lossy(&m);
                        self.outer.dm(vec![],&[NodeId::new(m.parse::<SocketAddr>().unwrap(), LocalNodeId::new(0))],true);
                    }
                }
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
