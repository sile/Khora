// #![allow(dead_code)] //failed attempt to disable warnings
// #![allow(non_snake_case)]
use kora::account::*;
use curve25519_dalek::scalar::Scalar;
use std::collections::VecDeque;
use std::time::Instant;
use kora::transaction::*;
use curve25519_dalek::ristretto::{CompressedRistretto};
use sha3::{Digest, Sha3_512};
use rayon::prelude::*;
use kora::randblock::*;
use kora::bloom::*;
use kora::validation::*;
use kora::constants::PEDERSEN_H;
/*
cargo run --bin please_work --release
*/

/* Anonymity ring size: proof size = 2*ceil(log_2 (3+|R|+|R||S|+β|T|+3|S|))+9... */
// |R| = floor((2^n-3-β|T|-3|S|)/(1+|S|)) for maximum efficiency (constant inner product proof size)
// we'd make 2^n = 128 or something REMEMBER TO SWITCH TO FLOAT FOR THIS... we cant do max efficiency with poly predefine
// we're going with a bit less than 128, β = 64
// there's an upper bound on the number of outputs (page 6)
// no one can EVER have more than 264 uwus of monies

/* the network will shard it's validation when there's enough validators. gives poorer people an opportunity. */



/* do i need to check if tags are uniue in the tx if i know ring is unique? */
/* make new txt file for saving tx and stuff when speanding anything */
/* tx sent to vals indevidually not as block (avoid denial of survace attacks) */
/* how the staker info is saved is totally off blockchain so i can change it if i want to later */
fn main() -> Result<(),std::io::Error> {
    let code_start = Instant::now();
    println!("Starting!!! >(^.^)>");
    // let n = 2u64; // just checking ceil(2) = 2
    // println!("{:#?}",n.next_power_of_two());

    let u = format!("{}",0);
    let gabrial = Account::new(&u); //make a new account
    let w = format!("{}",1);
    let ryan = Account::new(&w); //make a new account
    let x = format!("{}",2);
    let constantine = Account::new(&x); //make a new account
    let y = format!("{}",3);
    let kimberly = Account::new(&y); //make a new account
    


    /* lets not directly say hardware requirements, do suggestions that evolve over time */
    /* etherium has comittes of 128 or more */
    /* if the leader makes multiple blocks, they get slashed */
    let tx_processed = 16usize;
    let max_shards = 64usize; /* this if for teting purposes... there IS NO MAX SHARDS */
    

    let txvec = random_tx_set(&tx_processed);
    let lkey = Scalar::from(0u64);
    let leader = (lkey*PEDERSEN_H()).compress();
    let val_pool = (0..NUMBER_OF_VALIDATORS).into_par_iter().map(|x| (Scalar::from(x)*PEDERSEN_H()).compress()).collect::<Vec<CompressedRistretto>>();

    StakerState::initialize();
    History::initialize();
    BloomFile::initialize_bloom_file();
    let bloom = BloomFile::from_keys(1, 2); // everyone has different keys for this


    let start = Instant::now();
    Block::valicreate(&Scalar::from(1u8),&leader,&txvec,&0,&bloom);
    println!("time clean block: {:?} ms",start.elapsed().as_millis());
    let sigs = (0..(NUMBER_OF_VALIDATORS as u64)).into_par_iter().map(|x: u64| Block::valicreate(&Scalar::from(x),&leader,&txvec,&0,&bloom)).collect::<Vec<Block>>();
    let start = Instant::now();
    let block = Block::finish(&lkey, &sigs, &val_pool, &0);
    println!("time to complete block: {:?} ms",start.elapsed().as_millis());
    // let start = Instant::now();
    // block.verify(&val_pool).unwrap();
    // println!("time to verify block: {:?} ms",start.elapsed().as_millis());
    println!("tx: {:?}",block.txs.len());
    println!("validators per any shard: {:?}",NUMBER_OF_VALIDATORS);
    println!("shard validators: {:?}",block.shards.len());
    println!("full block: {} bytes",bincode::serialize(&block).unwrap().len());
    println!("block 0 done (unverified and unchecked transactions inside)!");
    println!("-------------------------------->");


    let info = block.scan_as_noone();
    History::append(&info.txout);
    let mut history = info.txout;
    let mut stkinfo = info.stkin;
    println!("stakers: {:?}",stkinfo.len());

    
    // println!("{:?}",comittee);
    // println!("{:?}",queue);

    // println!("stakers: {:?}",stkinfo);

    // for i in 0..4 {
    //     let mut mine = Vec::<(u64,OTAccount)>::new(); // read from file
    //     let mut lastheight = 0u64;
    //     let mut height = 0u64;
    //     block.scan(&Account::new(&format!("{}",i)), &mut mine, &mut height);
    //     println!("{}'s at {:?}",i,mine.par_iter().map(|x|x.0).collect::<Vec<_>>());
    // }
    let mut mine = Vec::<(u64,OTAccount)>::new(); // read from file
    let mut lastheight = 0u64;
    let mut height = 0u64;
    block.scan(&ryan, &mut mine, &mut height);
    println!("I'm at {:?}",mine.par_iter().map(|x|x.0).collect::<Vec<_>>());
    let mut smine = Vec::<[u64;2]>::new(); // read from file
    let mut sheight = 0u64;
    block.stkscan(&ryan, &mut smine, &mut sheight);
    println!("my stk accs: {:?}",smine.len());


    // println!("I exit being a staker");
    // let mut txvec = random_polytx_set(&tx_processed, &history, &lastheight);
    // let txsleave = Transaction::spend_ring(&vec![smine[0].1.to_owned()], &vec![]).polyform(&smine[0].0.to_le_bytes().to_vec());
    // txsleave.verifystk(&stkinfo).unwrap();// all to fee so shouldn't mess up my ordering with the randblock (but mught with comittee)
    // txvec.push(txsleave);



    

    // println!("{:?}",bincode::serialize(&mine[0].1.eek).unwrap().len());
    // println!("{:?}",bincode::serialize(&mine[0].1.eck).unwrap().len());
    // println!("{:?}",bincode::serialize(&mine[0].1).unwrap().len());
    // println!("{:?}",bincode::serialize(&block.txs[0].outputs[0]).unwrap().len());
    // println!("{:?}",bincode::serialize(&block.txs[0].outputs[0].eek).unwrap().len());
    // println!("{:?}",bincode::serialize(&block.txs[0].outputs[0].eck).unwrap().len());

    // let rname = generate_ring(&mine.par_iter().map(|(x,_)|*x as usize).collect::<Vec<usize>>(), &15, &height);
    // let ring = recieve_ring(&rname);
    // /* this is where people send you the ring members */ 
    // let mut rlring = ring.into_par_iter().map(|x| history[x as usize].to_owned()).collect::<Vec<OTAccount>>();
    // /* this is where people send you the ring members */ 
    // rlring.par_iter_mut().for_each(|x|if let Ok(y)=ryan.receive_ot(&x.clone()) {*x = y; println!("{:?}",x.tag);});
    // let x = Transaction::spend_ring(&rlring,&vec![(&constantine,&Scalar::from(1u8))]);
    // x.verify().unwrap();
    // let x = x.polyform(&rname);
    // x.verify(&history).unwrap();
    // println!("{:?}",x.inputs);






    let mut queue = (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>();
    let mut exitqueue = (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<VecDeque<usize>>()).collect::<Vec<_>>();
    let mut comittee = (0..max_shards).map(|_|(0..NUMBER_OF_VALIDATORS as usize).collect::<Vec<usize>>()).collect::<Vec<_>>();
    let mut alltagsever = Vec::<CompressedRistretto>::new();
    let mut nextblock = NextBlock::default();


    let iterations = 3;

    let mut bnum = 0u64;
    for _ in 0..iterations { /* there's a lot less new money the random number generator is fine */
        let shards = 2u64.pow(bnum as u32) as usize; /* max of 512 shard without lazyness because number of validators fits inside a u16 */
        let tx_per_shard = tx_processed/shards;
        bnum+=1;
        let start = Instant::now();
        let mut hasher = Sha3_512::new();
        hasher.update(&bincode::serialize(&nextblock).unwrap());
        let last_name = Scalar::from_hash(hasher.clone()).as_bytes().to_vec();
        println!("time to hash last block: {:?} ms",start.elapsed().as_millis());
        for i in 0..max_shards {
            select_stakers(&last_name,&(i as u128),&mut queue[i], &mut exitqueue[i],&mut comittee[i],&stkinfo);
        }
        println!("comittee 0: {:?}",comittee[0]);
        println!("queue 0: {:?}",queue[0]);
        let vals = comittee.par_iter().map(|y|y.par_iter().map(|x| {
            let a = Account::new(&format!("{}",x%(tx_processed/2)));
            let b = a.derive_stk_ot(&Scalar::from(stkinfo[*x].1));
            a.receive_ot(&b).unwrap().sk.unwrap()
        }).collect::<Vec<Scalar>>()).collect::<Vec<Vec<Scalar>>>();
        let val_pool = comittee.par_iter().map(|y|y.par_iter().map(|x| *x as u64).collect::<Vec<u64>>()).collect::<Vec<Vec<u64>>>();

        let lkey = vals[0][2];
        let leader = (vals[0][2]*PEDERSEN_H()).compress();
        let leader_loc = comittee[0][2] as u64; /* need to change all the signing to H not G */

        let mut txvec = random_polytx_set(&tx_processed, &history, &lastheight);

        
        if bnum == iterations {
            println!("I exit being a staker on this final turn");
            println!("stk loc: {:?}",smine[0][0]);
            println!("stk amount: {:?}",smine[0][1]);
            println!("stk both: {:?}",stkinfo[smine[0][0] as usize]);
            let txleave = Transaction::spend_ring(&vec![ryan.stake_acc().receive_ot(&ryan.stake_acc().derive_stk_ot(&Scalar::from(smine[0][1]))).unwrap()], &vec![]);
            txleave.verify().unwrap();
            println!("passed test 1");
            let txleave = txleave.polyform(&smine[0][0].to_le_bytes().to_vec());
            txleave.verifystk(&stkinfo).unwrap();// all to fee so shouldn't mess up my ordering with the randblock (but mught with comittee)
            txvec[0] = txleave;
        }

        let txvec = txvec.par_chunks(tx_per_shard).collect::<Vec<&[PolynomialTransaction]>>();
        let mut shardblocks = Vec::<NextBlock>::new();
        for i in 0..shards {
            let start = Instant::now();
            NextBlock::valicreate(&vals[i][0], &(comittee[i][0] as u64),&leader,&txvec[i].to_vec(),&(i as u16), &bnum,&last_name,&bloom,&history,&stkinfo);
            println!("time clean shard {}: {:?} ms",i,start.elapsed().as_millis());
            let sigs = vals[i].clone().into_par_iter().zip(comittee[i].clone()).map(|(x,l)| NextBlock::valicreate(&x,&(l as u64), &leader,&txvec[i].to_vec(),&(i as u16), &bnum,&last_name,&bloom,&history,&stkinfo)).collect::<Vec<NextBlock>>();
            let start = Instant::now();
            let block = NextBlock::finish(&lkey, &leader_loc, &sigs, &val_pool[i], &(i as u16), &bnum,&last_name,&stkinfo);
            println!("time to complete shard {}: {:?} ms",i,start.elapsed().as_millis());
            shardblocks.push(block);
        }
        if shards > 1 {
            let pool_nums = (0..shards).map(|x| x as u16).collect::<Vec<u16>>();
            let start = Instant::now();
            NextBlock::valimerge(&vals[0][0], &(comittee[0][0] as u64),&leader,&shardblocks,&val_pool,&pool_nums, &bnum,&last_name,&stkinfo);
            println!("time merge next block: {:?} ms",start.elapsed().as_millis());
            let sigs = vals[0].clone().into_par_iter().zip(val_pool[0].clone()).map(|(x,l)| NextBlock::valimerge(&x, &(l as u64),&leader,&shardblocks,&val_pool,&pool_nums,&bnum,&last_name,&stkinfo)).collect::<Vec<Signature>>();
            let start = Instant::now();
            nextblock = NextBlock::finishmerge(&lkey, &leader_loc, &sigs, &shardblocks, &val_pool, &val_pool[0], &pool_nums, &bnum,&last_name,&stkinfo);
            println!("time to complete next block: {:?} ms (runs concurrently to time to merge block because leader merges independantly)",start.elapsed().as_millis());
        }
        else {
            nextblock = shardblocks[0].to_owned();
        }

        let start = Instant::now();
        nextblock.verify(&val_pool[0],&stkinfo).unwrap();
        println!("time to verify next block: {:?} ms",start.elapsed().as_millis());
        let start = Instant::now();
        nextblock.tolightning().verify(&val_pool[0],&stkinfo).unwrap();
        println!("time to verify lightning: {:?} ms",start.elapsed().as_millis());
        println!("tx: {:?}",nextblock.txs.len());
        println!("validators per any shard: {:?}",NUMBER_OF_VALIDATORS);
        println!("shard validators: {:?}",nextblock.shards.len());
        println!("full block: {} bytes",bincode::serialize(&nextblock).unwrap().len());
        println!("lightning block: {} bytes",bincode::serialize(&nextblock.tolightning()).unwrap().len());


        // StakerState::replace(&stkinfo);
        // stkinfo = StakerState::read();

        lastheight = height;
        nextblock.scan(&ryan, &mut mine, &mut height, &mut alltagsever);
        nextblock.scanstk(&ryan, &mut smine, &mut sheight, &val_pool[0]);
        nextblock.scan_as_noone(&mut history,&mut stkinfo,&val_pool);
        println!("history: {}",history.len());
        println!("stkinfo: {}",stkinfo.len());
        println!("-------------------------------->"); /* right now, bloom filter filters staker exits? */
        nextblock.update_bloom(&bloom);
    }

    // /* these next 2 lines are for if you dont want to store all the otaccounts. save a in a txt file and read location ___ */
    // let a = history.par_iter().map(|x|[x.pk.compress(),x.com.com.compress()]).collect::<Vec<[CompressedRistretto;2]>>();
    // do storage stuff
    // let b = a.par_iter().map(|z| OTAccount::summon_ota(z)).collect::<Vec<_>>();
    



























    // let start = Instant::now();
    // let a = fs::read("random_block1.txt".to_string()).unwrap();
    // let a = fs::read("saved/tags/bloom".to_string());
    // let mut file = BufReader::new(File::open(&"saved/tags/bloom".to_string()).unwrap());
    // let mut file = BufReader::new(File::open(&"saved/full_blocks/random_block1.txt".to_string()).unwrap());
    
    // let mut buffer = [0u8; 32];
    // file.seek(SeekFrom::Start(90)).expect("Seek failed");
    // file.read(&mut buffer);
    // println!("{:?}",buffer);
    // file.seek(SeekFrom::Start(90)).expect("Seek failed");
    // file.read(&mut buffer);
    // println!("{:?}",buffer);

    // {
    //     let mut f = File::create("saved/outputs/outputs.txt").unwrap();
    //     f.write_all(&[50u8; 32]);
    // }
    // let mut f = OpenOptions::new().append(true).open("saved/outputs/outputs.txt").unwrap();
    // f.write_all(&[74u8;32]);
    // println!("{:?}",f);
    // println!("{:?}",start.elapsed().as_millis());





    println!("Done!!! >(^.^)>{:?} ms",code_start.elapsed().as_millis());
    println!("Done!!! >(^.^)>{:?} s",code_start.elapsed().as_secs());
    println!("Done!!! >(^.^)>{:?} min",code_start.elapsed().as_secs()/60);
    Ok(())










    

}
