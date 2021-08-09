use curve25519_dalek::ristretto::{RistrettoPoint, CompressedRistretto};
use curve25519_dalek::scalar::Scalar;
use crate::commitment::Commitment;
use crate::account::*;
use rayon::{prelude::*, vec};
use crate::transaction::*;
use std::convert::{TryFrom, TryInto};
use crate::bloom::BloomFile;
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use rand::{thread_rng, Rng};
use sha2::{Digest, Sha256};
use sha3::{Sha3_512};
use crate::seal::SealSig;
use crate::lpke::Ciphertext;
use crate::external::inner_product_proof::InnerProductProof;
use ahash::AHasher;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::hash::Hasher;
use serde::{Serialize, Deserialize};
use std::collections::{HashSet, VecDeque};
use std::iter::FromIterator;
use crate::constants::PEDERSEN_H;

pub const NUMBER_OF_VALIDATORS: u16 = 16;
pub const REPLACERATE: usize = 4;
const BLOCK_KEYWORD: [u8;7] = [107,141,142,162,151,145,154]; // Gabriel in octal
pub const INFLATION_CONSTANT: u64 = 2u64.pow(40);


pub fn hash_to_scalar(message: &Vec<u8>) -> Scalar {
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Syncedtx{
    pub stkout: Vec<u64>,
    pub stkin: Vec<(CompressedRistretto,u64)>,
    pub txout: Vec<OTAccount>, // they delete this part individually after they realize it's not for them
    pub fees: u64,
}

impl Syncedtx { // more sent vs more calculated... do i store this in true block structure?
    pub fn from0(txs: &Vec<SavedTransactionFull>)->Syncedtx {
        let stkout = Vec::<u64>::new();
        let stkin = txs.par_iter().map(|x|
            x.outputs.par_iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some((z.pk.compress(),u64::from_le_bytes(z.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())))} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<(CompressedRistretto,u64)>>();
        let txout = txs.into_par_iter().map(|x|
            x.outputs.to_owned().into_par_iter().filter(|x| stakereader_acc().read_ot(x).is_err()).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        let fees = u64::from_le_bytes(txs.par_iter().map(|x|x.fee).sum::<Scalar>().as_bytes()[..8].try_into().unwrap());
        Syncedtx{stkout,stkin,txout,fees}
    }
    pub fn to_sign0(txs: &Vec<SavedTransactionFull>)->Vec<u8> {
        bincode::serialize(&Syncedtx::from0(txs)).unwrap()
    }
    pub fn from(txs: &Vec<PolynomialTransaction>)->Syncedtx {
        let stkout = txs.par_iter().filter_map(|x|
            if x.inputs.len() == 8 {Some(u64::from_le_bytes(x.inputs.to_owned().try_into().unwrap()))} else {None}
        ).collect::<Vec<u64>>();
        let stkin = txs.par_iter().map(|x|
            x.outputs.par_iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some((z.pk.compress(),u64::from_le_bytes(z.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())))} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<(CompressedRistretto,u64)>>();
        let txout = txs.into_par_iter().map(|x|
            x.outputs.to_owned().into_par_iter().filter(|x| stakereader_acc().read_ot(x).is_err()).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        let fees = u64::from_le_bytes(txs.par_iter().map(|x|x.fee).sum::<Scalar>().as_bytes()[..8].try_into().unwrap());
        Syncedtx{stkout,stkin,txout,fees}
    }
    pub fn to_sign(txs: &Vec<PolynomialTransaction>)->Vec<u8> {
        bincode::serialize(&Syncedtx::from(txs)).unwrap()
    }
}


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Signature0{
    pub c: Scalar,
    pub r: Scalar,
    pub pk: CompressedRistretto,
}

impl Signature0 {
    pub fn sign(key: &Scalar, message: &Vec<Vec<u8>>) -> Signature0 {
        let message = message.into_par_iter().flatten().map(|&x| x).collect::<Vec<u8>>();
        let mut s = Sha256::new();
        s.update(&message);
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        s.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = s.finalize();
        let c = Scalar::from_bytes_mod_order(c.to_vec().try_into().unwrap());
        Signature0{c, r: (a - c*key), pk: (key*PEDERSEN_H()).compress()}
    }
    pub fn verify(&self, message: &Vec<Vec<u8>>) -> bool {
        let message = message.into_par_iter().flatten().map(|&x| x).collect::<Vec<u8>>();
        let mut s = Sha256::new();
        s.update(&message);
        s.update((self.r*PEDERSEN_H() + self.c*self.pk.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_bytes_mod_order(s.finalize().to_vec().try_into().unwrap())
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Block {
    pub validators: Vec<Signature0>,
    pub shards: Vec<CompressedRistretto>,
    pub leader: Signature0,
    pub txs: Vec<SavedTransactionFull>,
    pub bnum: u64,
    pub forker: Option<([Signature0;2],[Vec<u8>;2],u64)>,
} /* do they need to sign the hash of the previous block? */
impl Block { // need to sign the staker inputs too
    pub fn valicreate(key: &Scalar, leader: &CompressedRistretto, txs: &Vec<Transaction>, bnum: &u64, bloom: &BloomFile) -> Block {
        // let txs = // i don't care about block 0
        //     txs.into_par_iter().enumerate().filter_map(|(i,x)| {
        //         if 
        //         x.tags.par_iter().all(|&x|
        //             txs[..i].par_iter().flat_map(|x| x.tags.clone()).collect::<Vec<CompressedRistretto>>()
        //             .par_iter().all(|&y| // i do flatten it a lot
        //             x != y))
        //         & // only need to check tags for non stk btw
        //         x.tags.par_iter().all(|y| !bloom.contains(&y.to_bytes()))
        //         &
        //         x.tags.par_iter().enumerate().all(|(i,y)|
        //             x.tags[..i].par_iter().all(|z| {y!=z}
        //         ))
        //         &
        //         x.verify().is_ok()
        //         {
        //             Some(x.to_owned())
        //         }
        //         else {None}
        //     }).collect::<Vec<Transaction>>(); /* input of 1 tx -> it's from a staker*/
        let txs = txs.par_iter().map(|x|SavedTransactionFull::from(x)).collect::<Vec<SavedTransactionFull>>();
        Block {
            validators: vec![],
            shards: vec![],
            leader: Signature0::sign(&key,&vec![leader.to_bytes().to_vec(),Syncedtx::to_sign0(&txs),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec()]),
            txs: txs.to_owned(),
            bnum: *bnum,
            forker: None,
        }
    }
    pub fn finish(key: &Scalar, sigs: &Vec<Block>, validator_pool: &Vec<CompressedRistretto>, bnum: &u64) -> Block {
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let sigs = sigs.into_par_iter().filter(|x| !validator_pool.into_par_iter().all(|y| x.leader.pk != *y)).map(|x| x.to_owned()).collect::<Vec<Block>>();
        let mut sigs = sigs.par_iter().enumerate().filter_map(|(i,x)| if sigs[..i].par_iter().all(|y| x.leader.pk != y.leader.pk) {Some(x.to_owned())} else {None}).collect::<Vec<Block>>();
        let mut txs = Vec::<SavedTransactionFull>::new();
        let mut sigfinale = Vec::<Block>::new();
        for _ in 0..(sigs.len() as u16 - 2*NUMBER_OF_VALIDATORS/3) {
            let b = sigs.pop().unwrap();
            txs = b.txs;
            sigfinale = sigs.par_iter().filter(|x| if let (Ok(z),Ok(y)) = (bincode::serialize(&x.txs),bincode::serialize(&txs)) {z==y} else {false}).map(|x| x.to_owned()).collect::<Vec<Block>>();
            if sigfinale.len() as u16 > 2*NUMBER_OF_VALIDATORS/3 {
                break;
            }
        }
        let shortcut = Syncedtx::to_sign0(&sigfinale[0].txs); /* moving this to after the for loop made this code 3x faster. this is just a reminder to optimize everything later. (i can use [0]) */
        let sigfinale = sigfinale.into_par_iter().filter(|x| Signature0::verify(&x.leader, &vec![leader.clone(),shortcut.to_owned(),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec()])).map(|x| x.to_owned()).collect::<Vec<Block>>();
        /* my txt file codes are hella optimized but also hella incomplete with respect to polynomials and also hella disorganized */
        let sigs = sigfinale.par_iter().map(|x| x.leader.to_owned()).collect::<Vec<Signature0>>();
        
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();
        let leader = Signature0::sign(&key, &vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(),c.to_vec(),bincode::serialize(&None::<([Signature0;2],[Vec<u8>;2],u64)>).unwrap()]);
        Block{validators: sigs, shards: vec![], leader, txs, bnum: bnum.to_owned(), forker: None}
    }
    pub fn valimerge(key: &Scalar, leader: &CompressedRistretto, blks: &Vec<Block>, val_pools: &Vec<Vec<CompressedRistretto>>, bnum: &u64) -> Signature0 {
        let mut blks: Vec<Block> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<CompressedRistretto>>());
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks {
            if b.bnum == *bnum {
                b.txs = b.txs.into_par_iter().filter(|t| {
                    // tags.is_disjoint(&HashSet::from_par_iter(t.tags.par_iter().cloned()))
                    t.tags.par_iter().all(|x| !tags.contains(x))
                }).collect::<Vec<SavedTransactionFull>>();
                blk.txs.par_extend(b.txs.clone());
                let x = b.txs.len();
                tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
                if x > 64 {
                    blk.shards.par_extend(b.shards);
                    blk.shards.par_extend(b.validators.into_par_iter().map(|x| x.pk).collect::<Vec<CompressedRistretto>>());
                }
            }
        }
        let s = blk.shards.clone();
        blk.shards = blk.shards.par_iter().enumerate().filter_map(|(i,x)| if s[..i].par_iter().all(|y| x != y) {Some(x.to_owned())} else {None}).collect::<Vec<CompressedRistretto>>();
        // blk.shards = blk.shards.into_par_iter().collect::<HashSet<CompressedRistretto>>().into_par_iter().collect::<Vec<CompressedRistretto>>(); /* marginally slower */
        Signature0::sign(&key,&vec![leader.to_bytes().to_vec(),Syncedtx::to_sign0(&blk.txs),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec()])
    }
    pub fn finishmerge(key: &Scalar, sigs: &Vec<Signature0>, blks: &Vec<Block>, val_pools: &Vec<Vec<CompressedRistretto>>, pool0: &Vec<CompressedRistretto>, bnum: &u64) -> Block {
        let mut blks: Vec<Block> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<CompressedRistretto>>());
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks {
            b.txs = b.txs.into_par_iter().filter(|t| {
                // tags.is_disjoint(&HashSet::from_par_iter(t.tags.par_iter().cloned()))
                t.tags.par_iter().all(|x| !tags.contains(x))
            }).collect::<Vec<SavedTransactionFull>>();
            blk.txs.par_extend(b.txs.clone());
            let x = b.txs.len();
            tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
            if x > 64 {
                blk.shards.par_extend(b.shards);
                blk.shards.par_extend(b.validators.into_par_iter().map(|x| x.pk).collect::<Vec<CompressedRistretto>>());
            }
        }
        let s = blk.shards.clone();
        blk.shards = blk.shards.par_iter().enumerate().filter_map(|(i,x)| if s[..i].par_iter().all(|y| x != y) {Some(x.to_owned())} else {None}).collect::<Vec<CompressedRistretto>>();
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let sigs = sigs.into_par_iter().filter(|x| Signature0::verify(x, &vec![leader.clone(),bincode::serialize(&blk.txs.par_iter().map(|x| x.outputs.to_owned()).flatten().collect::<Vec<OTAccount>>()).unwrap(),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec()])).map(|x| x.to_owned()).collect::<Vec<Signature0>>();
        let sigs = sigs.into_par_iter().filter_map(|x| if !pool0.into_par_iter().all(|y| x.pk != *y) {Some(x.clone())} else {None}).collect::<Vec<Signature0>>();
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();
        let leader = Signature0::sign(&key, &vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(),c.to_vec(),bincode::serialize(&blk.forker).unwrap()]);
        Block{validators: sigs, shards: blk.shards, leader, txs: blk.txs, bnum: bnum.to_owned(), forker: None}
    }
    pub fn verify(&self, validator_pool: &Vec<CompressedRistretto>) -> Result<bool, &'static str> {
        if let Some((s,v,b)) = &self.forker {
            if s[0].pk != s[1].pk {
                return Err("forker is not 1 person")
            }
            if (s[0].c == s[1].c) & (s[0].r == s[1].r) {
                return Err("forker is not a forker")
            }
            if !s[0].verify(&vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[0].to_owned()]) | !s[1].verify(&vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[1].to_owned()]) {
                return Err("the forker was framed")
            }
        }
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        if !self.leader.verify(&vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(),c.to_vec(),bincode::serialize(&self.forker).unwrap()]) {
            return Err("leader is fake")
        }
        if !self.validators.par_iter().all(|x| x.verify(&vec![self.leader.pk.as_bytes().to_vec().clone(),Syncedtx::to_sign0(&self.txs),bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec()])) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().all(|x| !validator_pool.into_par_iter().all(|y| x.pk != *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| !self.validators.par_iter().all(|y| x.to_owned() != &y.pk)).count() < (2*NUMBER_OF_VALIDATORS as usize/3) {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<CompressedRistretto>>();
        if !x.clone().par_iter().enumerate().all(|(i,y)| x.clone()[..i].into_par_iter().all(|z| y != z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }
    pub fn scan_as_noone(&self) -> Syncedtx {
        Syncedtx::from0(&self.txs)
    }
    pub fn scan(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64) -> Syncedtx {
        let x = Syncedtx::from0(&self.txs); // will need to include something for stker amnt update
        mine.par_extend(x.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>());
        *height += x.txout.len() as u64; // probably going to do similar thing for stk
        x
    }
    pub fn stkscan(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64) {
        let stkin = self.txs.par_iter().map(|x|
            x.outputs.par_iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some(z)} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        
        mine.par_extend(stkin.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.stake_acc().receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>());
        *height += stkin.len() as u64; // probably going to do similar thing for stk
    }
    // pub fn tofastsync(&self) -> LightningSyncBlock {
    //     LightningSyncBlock {
    //         validators: self.validators.to_owned(),
    //         shards: self.shards.to_owned(),
    //         leader: self.leader.to_owned(),
    //         txs: self.txs.to_owned().into_par_iter().map(|x| x.shorten()).flatten().collect::<Vec<OTAccount>>(),
    //         bnum: self.bnum.to_owned(),
    //         forker: self.forker.to_owned(),
    //     }
    // }
}


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Signature{
    pub c: Scalar,
    pub r: Scalar,
    pub pk: u64,
}

impl Signature {
    pub fn sign(key: &Scalar, message: &Vec<u8>, location: &u64) -> Signature {
        // let message = vec![32u8,64u8];
        let mut s = Sha256::new();
        s.update(&message);
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        s.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = s.finalize();
        let c = Scalar::from_bytes_mod_order(c.to_vec().try_into().unwrap());
        Signature{c, r: (a - c*key), pk: *location}
    }
    pub fn verify(&self, message: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> bool {
        // let message = vec![32u8,64u8];
        let mut s = Sha256::new();
        s.update(&message);
        s.update((self.r*PEDERSEN_H() + self.c*stkstate[self.pk as usize].0.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_bytes_mod_order(s.finalize().to_vec().try_into().unwrap())
    }
}


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct NextBlock {
    pub validators: Vec<Signature>,
    pub shards: Vec<u64>,
    pub leader: Signature,
    pub txs: Vec<PolynomialTransaction>,
    pub last_name: Vec<u8>,
    pub bnum: u64,
    pub forker: Option<([Signature;2],[Vec<u8>;2],u64)>,
}
impl NextBlock { // need to sign the staker inputs too
    pub fn valicreate(key: &Scalar, location: &u64, leader: &CompressedRistretto, txs: &Vec<PolynomialTransaction>, bnum: &u64, last_name: &Vec<u8>, bloom: &BloomFile, history: &Vec<OTAccount>, stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock {
        let mut stks = txs.par_iter().filter_map(|x| 
            if x.inputs.len() == 8 {if x.verifystk(&stkstate).is_ok() {Some(x.to_owned())} else {None}} else {None}
        ).collect::<Vec<PolynomialTransaction>>(); /* i would use drain_filter but its unstable */
        let txs = txs.into_par_iter().filter_map(|x| 
            if x.inputs.len() != 8 {Some(x.to_owned())} else {None}
        ).collect::<Vec<PolynomialTransaction>>();
        
        // let txscopy = txs; // maybe make this Arc<RwLock> to allow multiple reads without cloning?
        let mut txs =
            txs.clone().into_par_iter().enumerate().filter_map(|(i,x)| {
                if 
                x.tags.par_iter().all(|&x|
                    txs[..i].par_iter().flat_map(|x| x.tags.clone()).collect::<Vec<CompressedRistretto>>()
                    .par_iter().all(|&y|
                    x != y)) /* should i replace this with a bloom filter or hashset??? and not parallelize it? */
                &
                x.tags.par_iter().all(|y| !bloom.contains(&y.to_bytes()))
                &
                x.tags.par_iter().enumerate().all(|(i,y)|
                    x.tags[..i].par_iter().all(|z| {y!=z}
                ))
                &
                x.verify(&history).is_ok()
                {
                    Some(x.to_owned())
                }
                else {None}
        }).collect::<Vec<PolynomialTransaction>>(); /* input of 1 tx -> it's from a staker */
        txs.append(&mut stks);


        let m = vec![leader.to_bytes().to_vec(),Syncedtx::to_sign(&txs),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        NextBlock {
            validators: vec![],
            shards: vec![],
            leader: Signature::sign(&key,&m,&location),
            txs: txs.to_owned(),
            last_name: last_name.to_owned(),
            bnum: *bnum,
            forker: None,
        }
    }
    pub fn finish(key: &Scalar, location: &u64, sigs: &Vec<NextBlock>, validator_pool: &Vec<u64>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock { // <----do i need to reference previous block explicitly?
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let mut sigs = sigs.into_par_iter().filter(|x| !validator_pool.into_par_iter().all(|y| x.leader.pk != *y)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
        let mut txs = Vec::<PolynomialTransaction>::new();
        let mut sigfinale = Vec::<NextBlock>::new();
        for _ in 0..(sigs.len() as u16 - 2*NUMBER_OF_VALIDATORS/3) {
            let b = sigs.pop().unwrap();
            txs = b.txs;
            sigfinale = sigs.par_iter().filter(|x| if let (Ok(z),Ok(y)) = (bincode::serialize(&x.txs),bincode::serialize(&txs)) {z==y} else {false}).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
            if sigfinale.len() as u16 > 2*NUMBER_OF_VALIDATORS/3 {
                break; /* this fails if most sigs are repeats i think */
            }
        } /* based on th eline below, a validator could send a ton of requests with fake tx and eliminate block making */
        let sigfinale = sigfinale.par_iter().enumerate().filter_map(|(i,x)| if sigs[..i].par_iter().all(|y| x.leader.pk != y.leader.pk) {Some(x.to_owned())} else {None}).collect::<Vec<NextBlock>>();
        /* literally just ignore block content with less than 2/3 sigs */
        let shortcut = Syncedtx::to_sign(&sigfinale[0].txs); /* moving this to after the for loop made this code 3x faster. this is just a reminder to optimize everything later. (i can use [0]) */
        let m = vec![leader.clone(),shortcut.to_owned(),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let sigfinale = sigfinale.into_par_iter().filter(|x| Signature::verify(&x.leader, &m,&stkstate)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
        /* my txt file codes are hella optimized but also hella incomplete with respect to polynomials and also hella disorganized */
        let sigs = sigfinale.par_iter().map(|x| x.leader.to_owned()).collect::<Vec<Signature>>();
        /* do i need to add an empty block option that requires >1/3 signatures of you as leader? */
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(),last_name.clone(),c.to_vec(),bincode::serialize(&None::<([Signature;2],[Vec<u8>;2],u64)>).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
        let leader = Signature::sign(&key, &m,&location);
        
        NextBlock{validators: sigs, shards: vec![], leader, txs, last_name: last_name.to_owned(), bnum: bnum.to_owned(), forker: None}
    }
    pub fn valimerge(key: &Scalar, location: &u64, leader: &CompressedRistretto, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Signature {
        let mut blks: Vec<NextBlock> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y,&stkstate).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>());
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks {
            if b.bnum == *bnum {
                b.txs = b.txs.into_par_iter().filter(|t| {
                    // tags.is_disjoint(&HashSet::from_par_iter(t.tags.par_iter().cloned()))
                    t.tags.par_iter().all(|x| !tags.contains(x))
                }).collect::<Vec<PolynomialTransaction>>();
                blk.txs.par_extend(b.txs.clone());
                let x = b.txs.len();
                tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
                if x > 64 {
                    blk.shards.par_extend(b.shards);
                    blk.shards.par_extend(b.validators.into_par_iter().map(|x| x.pk).collect::<Vec<u64>>());
                }
            }
        }
        let s = blk.shards.clone();
        blk.shards = blk.shards.par_iter().enumerate().filter_map(|(i,x)| if s[..i].par_iter().all(|y| stkstate[*x as usize].0 != stkstate[*y as usize].0) {Some(x.to_owned())} else {None}).collect::<Vec<u64>>();
        


        let m = vec![leader.to_bytes().to_vec(),Syncedtx::to_sign(&blk.txs),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        Signature::sign(&key,&m, &location)
    }
    pub fn finishmerge(key: &Scalar, location: &u64, sigs: &Vec<Signature>, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, pool0: &Vec<u64>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock {
        let pool0 = pool0.into_par_iter().map(|x|stkstate[*x as usize].0).collect::<Vec<CompressedRistretto>>();
        let mut blks: Vec<NextBlock> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y,&stkstate).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>());
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        for mut b in blks {
            b.txs = b.txs.into_par_iter().filter(|t| {
                // tags.is_disjoint(&HashSet::from_par_iter(t.tags.par_iter().cloned()))
                t.tags.par_iter().all(|x| !tags.contains(x))
            }).collect::<Vec<PolynomialTransaction>>();
            blk.txs.par_extend(b.txs.clone());
            let x = b.txs.len();
            tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
            if x > 64 {
                blk.shards.par_extend(b.shards);
                blk.shards.par_extend(b.validators.into_par_iter().map(|x| x.pk).collect::<Vec<u64>>());
            }
        }
        let s = blk.shards.clone();
        // blk.shards = blk.shards.par_iter().enumerate().filter_map(|(i,x)| if s[..i].par_iter().all(|y| stkstate[*x as usize].0 != stkstate[*y as usize].0) {Some(x.to_owned())} else {None}).collect::<Vec<u64>>();
        blk.shards = blk.shards.into_par_iter().enumerate().filter(|(i,x)| s[..*i].par_iter().all(|y| stkstate[*x as usize].0 != stkstate[*y as usize].0)).map(|(_,x)|x).collect::<Vec<u64>>();
        
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let m = vec![leader.clone(),Syncedtx::to_sign(&blk.txs),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec(),last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let sigs = sigs.into_par_iter().filter(|x|
            Signature::verify(x, &m, stkstate)
        ).collect::<Vec<&Signature>>();
        // let sigs = sigs.into_par_iter().filter_map(|x| if !pool0.clone().into_par_iter().all(|y| stkstate[x.pk as usize].0 != y) {Some(x.clone())} else {None}).collect::<Vec<Signature>>();
        let sigs = sigs.into_par_iter().filter(|x| !pool0.clone().into_par_iter().all(|y| stkstate[x.pk as usize].0 != y)).collect::<Vec<&Signature>>();
        let sigcopy = sigs.clone();
        let sigs = sigs.into_par_iter().enumerate().filter_map(|(i,x)| if sigcopy[..i].par_iter().all(|y| x.pk != y.pk) {Some(x.to_owned())} else {None}).collect::<Vec<Signature>>();
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();

        let m = vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(), last_name.clone(),c.to_vec(),bincode::serialize(&blk.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
        let leader = Signature::sign(&key, &m, &location);
        NextBlock{validators: sigs.to_owned(), shards: blk.shards, leader, txs: blk.txs, last_name: last_name.clone(), bnum: bnum.to_owned(), forker: None}
    }
    pub fn verify(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        if let Some((s,v,b)) = &self.forker {
            if s[0].pk != s[1].pk { /* leader could cause a fork by messing with fork section too, not just who signs */
                return Err("forker is not 1 person")
            }
            if (s[0].c == s[1].c) & (s[0].r == s[1].r) {
                return Err("forker is not a forker")
            }
            let x = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[0].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            let y = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[1].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            if !s[0].verify(&x, &stkstate) | !s[1].verify(&y, &stkstate) {
                return Err("the forker was framed")
            }
        }
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec(),bincode::serialize(&self.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
        if !self.leader.verify(&m, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(),Syncedtx::to_sign(&self.txs),bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        if !self.validators.par_iter().all(|x| x.verify(&m, &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().all(|x| !validator_pool.into_par_iter().all(|y| x.pk != *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| !self.validators.par_iter().all(|y| x.to_owned() != &y.pk)).count() < (2*NUMBER_OF_VALIDATORS as usize/3) {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>();
        if !x.clone().par_iter().enumerate().all(|(i,y)| x.clone()[..i].into_par_iter().all(|z| y != z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }
    pub fn scan_as_noone(&self,history: &mut Vec<OTAccount>,valinfo: &mut Vec<(CompressedRistretto,u64)>,val_pools: &Vec<u64>) {
        let mut info = Syncedtx::from(&self.txs);
        history.append(&mut info.txout);
        let fees = u64::from_le_bytes(self.txs.par_iter().map(|x|x.fee).sum::<Scalar>().as_bytes()[..8].try_into().unwrap());
        let profits = fees/(self.validators.len() as u64);
        let inflation = INFLATION_CONSTANT/self.bnum;
        for &v in val_pools { // font i need to look at the validators here?
            if self.validators.par_iter().all(|x|x.pk!=v) {
                valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
            }
            else {
                valinfo[v as usize].1 += profits+inflation;
            }
        }
        for x in info.stkout.iter().rev() {
            valinfo.remove(*x as usize);
        }
        valinfo.append(&mut info.stkin);
        if let Some(evil) = self.forker.to_owned() {
            let x = valinfo.par_iter().enumerate().filter_map(|x|
                if x.1.0 == valinfo[evil.0[0].pk as usize].0 {Some(x.0)} else {None}
            ).collect::<Vec<usize>>();
            for x in x.iter().rev() {
                valinfo.remove(*x as usize);
            }
        }
    }
    pub fn scan(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64, alltagsever: &mut Vec<CompressedRistretto>) -> Syncedtx {
        let x = Syncedtx::from(&self.txs);
        let newmine = x.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        let newtags = newmine.par_iter().map(|x|x.1.tag.unwrap()).collect::<Vec<CompressedRistretto>>();
        if newtags.par_iter().all(|x| !alltagsever.par_iter().all(|y|y!=x)) {
            println!("you got burnt (someone sent you faerie gold!)");
        }
        alltagsever.par_extend(&newtags);
        mine.par_extend(newmine);
        // println!("{}", x.txout.len());
        *height += x.txout.len() as u64; // probably going to do similar thing for stk
        // println!("{}",height);
        x
    }
    pub fn scanstk(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64, val_pools: &Vec<u64>) {
        let stkin = self.txs.par_iter().map(|x|
            x.outputs.par_iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some(z)} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        let mut a = stkin.par_iter().enumerate().filter_map(|(i,x)| if let Ok(x) = me.stake_acc().receive_ot(x) {Some((i as u64,x))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        mine.append(&mut a);
        
        
        let fees = u64::from_le_bytes(self.txs.par_iter().map(|x|x.fee).sum::<Scalar>().as_bytes()[..8].try_into().unwrap());
        let profits = fees/(self.validators.len() as u64);
        let inflation = INFLATION_CONSTANT/self.bnum;
        for &v in val_pools {
            for (i,m) in mine.clone().iter().enumerate() {
                if m.0 == v {
                    if self.validators.par_iter().all(|x|x.pk!=v) {
                        let delta = Scalar::from(u64::from_le_bytes(m.1.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())/1000);
                        mine[i].1.com.amount = Some(mine[i].1.com.amount.unwrap() - delta);
                        mine[i].1.com.com -= delta*RISTRETTO_BASEPOINT_POINT;
                    }
                    else {
                        // println!("{:?}",mine[i].1.com.amount);
                        let delta = Scalar::from(profits+inflation);
                        mine[i].1.com.amount = Some(mine[i].1.com.amount.unwrap() + delta);
                        mine[i].1.com.com += delta*RISTRETTO_BASEPOINT_POINT;
                        // println!("{:?}",mine[i].1.com.amount);
                    }
                }
            }
        }
        let x = Syncedtx::from(&self.txs.to_owned()).stkout;
        for (i,m) in mine.clone().iter().enumerate().rev() {
            for v in &x {
                if m.0 == *v {
                    mine.remove(i as usize);
                }
            }
        }
        *height += stkin.len() as u64;
    }
    pub fn update_bloom(&self,bloom:&BloomFile) {
        self.txs.par_iter().map(|x| x.tags.par_iter().map(|y| bloom.insert(y.as_bytes())).collect::<Vec<_>>()).collect::<Vec<_>>();
    }
    pub fn tolightning(&self) -> LightningSyncBlock {
        LightningSyncBlock {
            validators: self.validators.to_owned(),
            shards: self.shards.to_owned(),
            leader: self.leader.to_owned(),
            info: Syncedtx::from(&self.txs),
            bnum: self.bnum.to_owned(),
            last_name: self.last_name.to_owned(),
            forker: self.forker.to_owned(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct LightningSyncBlock {
    pub validators: Vec<Signature>,
    pub shards: Vec<u64>,
    pub leader: Signature,
    pub info: Syncedtx,
    pub bnum: u64,
    pub last_name: Vec<u8>,
    pub forker: Option<([Signature;2],[Vec<u8>;2],u64)>,
}
impl LightningSyncBlock {
    pub fn verify(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        if let Some((s,v,b)) = &self.forker {
            if s[0].pk != s[1].pk {
                return Err("forker is not 1 person")
            }
            if (s[0].c == s[1].c) & (s[0].r == s[1].r) {
                return Err("forker is not a forker")
            }
            let x = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[0].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            let y = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[1].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            if !s[0].verify(&x, &stkstate) | !s[1].verify(&y, &stkstate) {
                return Err("the forker was framed")
            }
        }
        let mut s = Sha256::new();
        s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
        let c = s.finalize();
        let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec(),bincode::serialize(&self.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
        if !self.leader.verify(&m, &stkstate) {
            return Err("leader is fake")
        }
        let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(),bincode::serialize(&self.info).unwrap(),bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        if !self.validators.par_iter().all(|x| x.verify(&m, &stkstate)) {
            return Err("at least 1 validator is fake")
        }
        if !self.validators.par_iter().all(|x| !validator_pool.into_par_iter().all(|y| x.pk != *y)) {
            return Err("at least 1 validator is not in the pool")
        }
        if validator_pool.par_iter().filter(|x| !self.validators.par_iter().all(|y| x.to_owned() != &y.pk)).count() < (2*NUMBER_OF_VALIDATORS as usize/3) {
            return Err("there aren't enough validators")
        }
        let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>();
        if !x.clone().par_iter().enumerate().all(|(i,y)| x.clone()[..i].into_par_iter().all(|z| y != z)) {
            return Err("there's multiple signatures from the same validator")
        }
        return Ok(true)
    }
    pub fn scan(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64, alltagsever: &mut Vec<CompressedRistretto>) -> Syncedtx {
        let x = self.info.clone();
        let newmine = x.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        let newtags = newmine.par_iter().map(|x|x.1.tag.unwrap()).collect::<Vec<CompressedRistretto>>();
        if newtags.par_iter().all(|x| !alltagsever.par_iter().all(|y|y!=x)) {
            println!("you got burnt (someone sent you faerie gold!)");
        }
        alltagsever.par_extend(&newtags);
        mine.par_extend(newmine);
        // println!("{}",height);
        // println!("{}", x.txout.len());
        *height += x.txout.len() as u64; // probably going to do similar thing for stk
        // println!("{}",height);
        x
    }
    pub fn scan_as_noone(&self,history: &mut Vec<OTAccount>,valinfo: &mut Vec<(CompressedRistretto,u64)>,val_pools: &Vec<u64>) {
        let mut info =self.info.clone();
        history.append(&mut info.txout);
        // self.validators; <---- will be number. I can add fee rewards by location
        let fees = info.fees;
        let profits = fees/(self.validators.len() as u64);
        let inflation = INFLATION_CONSTANT/self.bnum;
        for &v in val_pools { // font i need to look at the validators here?
            if self.validators.par_iter().all(|x|x.pk!=v) {
                valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
            }
            else {
                valinfo[v as usize].1 += profits+inflation;
            }
        }
        for x in info.stkout.iter().rev() {
            valinfo.remove(*x as usize);
        }
        valinfo.append(&mut info.stkin);
        if let Some(evil) = self.forker.to_owned() {
            let x = valinfo.par_iter().enumerate().filter_map(|x|
                if x.1.0 == valinfo[evil.0[0].pk as usize].0 {Some(x.0)} else {None}
            ).collect::<Vec<usize>>();
            for x in x.iter().rev() {
                valinfo.remove(*x as usize);
            }
        }
    }
}




pub fn select_stakers(block: &Vec<u8>, shard: &u128, queue: &mut VecDeque<usize>, comittee: &mut Vec<usize>, stkstate: &Vec<(CompressedRistretto,u64)>) {
    let (_pool,y): (Vec<CompressedRistretto>,Vec<u128>) = stkstate.into_par_iter().map(|(x,y)| (x.to_owned(),*y as u128)).unzip();
    let tot_stk: u128 = y.par_iter().sum(); /* initial queue will be 0 for all non0 shards... */
    // println!("average stake:     {}",tot_stk/(y.len() as u128));
    // println!("number of stakers: {}",y.len());
    // println!("new money:         {}",y[8..].par_iter().sum::<u128>());
    // println!("total stake:       {}",tot_stk);
    // println!("random drawn from: {}",u64::MAX);
    let mut s = AHasher::new_with_keys(0, *shard);
    s.write(&block);
    let mut winner = (0..REPLACERATE).collect::<Vec<usize>>().par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as u128;
        // println!("unmoded winner:    {}",c);
        let mut staker = (c%tot_stk) as i128;
        // println!("winner:            {}",staker);
        let mut w = 0;
        for (i,&j) in y.iter().enumerate() {
            staker -= j as i128;
            if staker <= 0
                {w = i; break;}
        };
        w
    }).collect::<VecDeque<usize>>();
    // println!("winners:           {:?}",winner);
    queue.append(&mut winner); // need to hardcode initial state
    let winner = queue.par_drain(..REPLACERATE).collect::<Vec<usize>>();
    // println!("winner people: {:?}",winner); // these 2 should run concurrently

    let mut s = AHasher::new_with_keys(1, *shard);
    s.write(&block);
    let loser = (0..REPLACERATE).collect::<Vec<usize>>().par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as usize;
        c%NUMBER_OF_VALIDATORS as usize
    }).collect::<Vec<usize>>();
    // println!("loser locations: {:?}",loser);
    for (i,j) in loser.iter().enumerate() {
        comittee[*j] = winner[i];
    }
}


