use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use crate::account::*;
use rayon::prelude::*;
use crate::transaction::*;
use std::convert::TryInto;
use std::iter::FromIterator;
use std::ops::MulAssign;
use crate::bloom::BloomFile;
use rand::{thread_rng};
use sha3::{Digest, Sha3_512};
use ahash::AHasher;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::rename;
use std::io::Read;
use std::io::Write;
use std::hash::Hasher;
use serde::{Serialize, Deserialize};
use std::collections::{HashSet, VecDeque, vec_deque};
use crate::constants::PEDERSEN_H;
use std::io::{Seek, SeekFrom, BufReader};//, BufWriter};


pub const NUMBER_OF_VALIDATORS: u8 = 128;
pub const REPLACERATE: usize = 2;
pub const BLOCK_KEYWORD: [u8;6] = [107,105,109,98,101,114]; // todo: make this something else (a obvious version of her name)
const NOT_BLOCK_KEYWORD: [u8;7] = [103,97,98,114,105,101,108]; // todo: make this something else (a obvious version of her name)
pub const INFLATION_CONSTANT: u64 = 2u64.pow(30);
pub const INFLATION_EXPONENT: u32 = 100;

pub fn hash_to_scalar(message: &Vec<u8>) -> Scalar {
    let mut hasher = Sha3_512::new();
    hasher.update(&message);
    Scalar::from_hash(hasher)
} /* this is for testing purposes. it is used to check if 2 long messages are identicle */

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Syncedtx{
    pub stkout: Vec<u64>,
    pub stkin: Vec<(CompressedRistretto,u64)>,
    pub txout: Vec<OTAccount>, // they delete this part individually after they realize it's not for them
    pub tags: Vec<CompressedRistretto>,
    pub fees: u64,
}

impl Syncedtx {
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
        let tags = txs.par_iter().map(|x|
            x.tags.clone()
        ).flatten().collect::<Vec<CompressedRistretto>>();
        let fees = txs.par_iter().map(|x|x.fee).sum::<u64>();
        Syncedtx{stkout,stkin,txout,tags,fees}
    }
    pub fn to_sign(txs: &Vec<PolynomialTransaction>)->Vec<u8> {
        bincode::serialize(&Syncedtx::from(txs)).unwrap()
    }
}


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct MultiSignature{
    pub x: CompressedRistretto,
    pub y: Scalar,
    pub pk: Vec<u64>, // whose not in it
}
impl MultiSignature {
    pub fn gen_group_x(key: &Scalar, bnum: &u64) -> CompressedRistretto { // WARNING: make a function to sign arbitrary messages when sending them for validators
        let bnum = bnum.to_le_bytes();
        let mut s = Sha3_512::new();
        s.update(&bnum);
        s.update(&key.as_bytes()[..]);
        let m = ((Scalar::from_hash(s))*RISTRETTO_BASEPOINT_POINT).compress();
        m
    }
    pub fn sum_group_x(x: &Vec<RistrettoPoint>) -> CompressedRistretto {
        x.into_par_iter().sum::<RistrettoPoint>().compress()
    }
    pub fn try_get_y(key: &Scalar, bnum: &u64, message: &Vec<u8>, xt: &CompressedRistretto) -> Scalar {
        let bnum = bnum.to_le_bytes();
        let mut s = Sha3_512::new();
        s.update(&bnum);
        s.update(&key.as_bytes());
        let r = Scalar::from_hash(s);

        let mut s = Sha3_512::new();
        s.update(&message);
        s.update(&xt.as_bytes());
        let e = Scalar::from_hash(s);
        let y = e*key+r;
        y
    }
    pub fn sum_group_y(y:& Vec<Scalar>) -> Scalar {
        y.into_par_iter().sum()
    }
    pub fn verify_group(yt: &Scalar, xt: &CompressedRistretto, message: &Vec<u8>, who: &Vec<CompressedRistretto>) -> bool {
        let mut s = Sha3_512::new();
        s.update(&message);
        s.update(&xt.as_bytes());
        let e = Scalar::from_hash(s);
        (yt*RISTRETTO_BASEPOINT_POINT).compress() == (xt.decompress().unwrap() + e*who.into_par_iter().map(|x|x.decompress().unwrap()).sum::<RistrettoPoint>()).compress()
    }
}



#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Signature{
    pub c: Scalar,
    pub r: Scalar,
    pub pk: u64,
}

impl Signature { // the inputs are the hashed messages you are checking for signatures on because it's faster for many messages.
    pub fn sign(key: &Scalar, message: &mut Sha3_512, location: &u64) -> Signature {
        // let mut s = Sha3_512::new();
        // s.update(&message);
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        message.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(message.to_owned());
        Signature{c, r: (a - c*key), pk: *location}
    }
    pub fn verify(&self, message: &mut Sha3_512, stkstate: &Vec<(CompressedRistretto,u64)>) -> bool {
        // let mut s = Sha3_512::new();
        // s.update(&message);
        message.update((self.r*PEDERSEN_H() + self.c*stkstate[self.pk as usize].0.decompress().unwrap()).compress().to_bytes());
        self.c == Scalar::from_hash(message.to_owned())
    }

    pub fn sign_message(key: &Scalar, message: &Vec<u8>, location: &u64) -> Vec<u8> {
        let mut s = Sha3_512::new();
        s.update(&message); // impliment non block check stuff for signatures
        let mut csprng = thread_rng();
        let a = Scalar::random(&mut csprng);
        s.update((a*PEDERSEN_H()).compress().to_bytes());
        let c = Scalar::from_hash(s.to_owned());
        let mut out = c.as_bytes().to_vec();
        out.par_extend((a - c*key).as_bytes());
        out.par_extend(location.to_le_bytes());
        out.par_extend(message);
        out
    }
    pub fn recieve_signed_message(signed_message: &mut Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Vec<u8> {
        let message = signed_message.par_drain(72..).collect::<Vec<_>>();
        let s = Signature{c: Scalar::from_bits(message[..32].try_into().unwrap()),
            r: Scalar::from_bits(message[32..64].try_into().unwrap()),
            pk: u64::from_le_bytes(message[64..72].try_into().unwrap())};
        
        let mut h = Sha3_512::new();
        h.update(&message);
        if s.verify(&mut h, stkstate) {
            message
        } else {
            vec![]
        }
    }



}


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct NextBlock {
    pub emptyness: MultiSignature,
    pub validators: Vec<Signature>,
    pub shards: Vec<u64>,
    pub leader: Signature,
    pub txs: Vec<PolynomialTransaction>,
    pub last_name: Vec<u8>,
    pub pools: Vec<u16>,
    pub bnum: u64,
    pub forker: Option<([Signature;2],[Vec<u8>;2],u64)>,
}
impl NextBlock { // need to sign the staker inputs too
    pub fn valicreate(key: &Scalar, location: &u64, leader: &CompressedRistretto, txs: &Vec<PolynomialTransaction>, pool: &u16, bnum: &u64, last_name: &Vec<u8>, bloom: &BloomFile,/* _history: &Vec<OTAccount>,*/ stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock {
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
                x.tags.par_iter().all(|y| {!bloom.contains(&y.to_bytes())})
                &
                x.tags.par_iter().enumerate().all(|(i,y)|
                    x.tags[..i].par_iter().all(|z| {y!=z}
                ))
                &
                x.verify().is_ok()
                // x.verify_ram(&history).is_ok()
                {//println!("{:?}",x.tags);
                    Some(x.to_owned())
                }
                else {None}
        }).collect::<Vec<PolynomialTransaction>>(); /* input of 1 tx -> it's from a staker */
        txs.append(&mut stks);


        let m = vec![leader.to_bytes().to_vec(),Syncedtx::to_sign(&txs),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        // println!("\n\nval: {:?}\n\n",hash_to_scalar(&m));
        // println!("\n\nval: {:?}\n\n",hash_to_scalar(&leader.to_bytes().to_vec()));
        // println!("\n\nval: {:?}\n\n",bnum);
        let mut s = Sha3_512::new();
        s.update(&m);
        NextBlock {
            emptyness: MultiSignature::default(),
            validators: vec![],
            shards: vec![],
            leader: Signature::sign(&key,&mut s,&location),
            txs: txs.to_owned(),
            last_name: last_name.to_owned(),
            pools: vec![*pool],
            bnum: *bnum,
            forker: None,
        }
    }
    pub fn finish(key: &Scalar, location: &u64, sigs: &Vec<NextBlock>, validator_pool: &Vec<u64>, pool: &u16, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock { // <----do i need to reference previous block explicitly?
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let mut sigs = sigs.into_par_iter().filter(|x| !validator_pool.into_par_iter().all(|y| x.leader.pk != *y)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
        let mut txs = Vec::<PolynomialTransaction>::new();
        let mut sigfinale: Vec<NextBlock>;
        for _ in 0..(sigs.len() as u8 - 2*(NUMBER_OF_VALIDATORS/3)) {
            let b = sigs.pop().unwrap();
            txs = b.txs;
            sigfinale = sigs.par_iter().filter(|x| if let (Ok(z),Ok(y)) = (bincode::serialize(&x.txs),bincode::serialize(&txs)) {z==y} else {false}).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
            if sigfinale.len() as u8 > 2*(NUMBER_OF_VALIDATORS/3) {
                let sigfinale = sigfinale.par_iter().enumerate().filter_map(|(i,x)| if sigs[..i].par_iter().all(|y| x.leader.pk != y.leader.pk) {Some(x.to_owned())} else {None}).collect::<Vec<NextBlock>>();
                // println!("{:?}",sigfinale.len());
                let shortcut = Syncedtx::to_sign(&sigfinale[0].txs); /* moving this to after the for loop made this code 3x faster. this is just a reminder to optimize everything later. (i can use [0]) */
                let m = vec![leader.clone(),shortcut.to_owned(),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
                // println!("\n\nled: {:?}\n\n",hash_to_scalar(&m));
                // println!("\n\nled: {:?}\n\n",hash_to_scalar(&leader));
                // println!("\n\nled: {:?}\n\n",bnum);
                // println!("{}",sigfinale.len()); // 1
                let mut s = Sha3_512::new();
                s.update(&m);
                let sigfinale = sigfinale.into_par_iter().filter(|x| Signature::verify(&x.leader, &mut s.clone(),&stkstate)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
                // println!("{}",sigfinale.len()); // 0
                /* my txt file codes are hella optimized but also hella incomplete with respect to polynomials and also hella disorganized */
                let sigs = sigfinale.par_iter().map(|x| x.leader.to_owned()).collect::<Vec<Signature>>();
                // println!("{}",sigs.len());
                /* do i need to add an empty block option that requires >1/3 signatures of you as leader? */
                let mut s = Sha3_512::new();
                s.update(&bincode::serialize(&sigs).unwrap().to_vec());
                let c = s.finalize();
                let m = vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(),last_name.clone(),c.to_vec(),bincode::serialize(&None::<([Signature;2],[Vec<u8>;2],u64)>).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
                let mut s = Sha3_512::new();
                s.update(&m);
                let leader = Signature::sign(&key, &mut s,&location);
                
                return NextBlock{emptyness: MultiSignature::default(), validators: sigs, shards: vec![], leader, txs, last_name: last_name.to_owned(), pools: vec![*pool], bnum: bnum.to_owned(), forker: None}
            }
        } /* based on the line below, a validator could send a ton of requests with fake tx and eliminate block making */
        sigfinale = sigs.par_iter().filter(|x| if let (Ok(z),Ok(y)) = (bincode::serialize(&x.txs),bincode::serialize(&Vec::<PolynomialTransaction>::new())) {z==y} else {false}).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
        if sigfinale.len() as u8 > (NUMBER_OF_VALIDATORS/2) {
            let sigfinale = sigfinale.par_iter().enumerate().filter_map(|(i,x)| if sigs[..i].par_iter().all(|y| x.leader.pk != y.leader.pk) {Some(x.to_owned())} else {None}).collect::<Vec<NextBlock>>();
            /* literally just ignore block content with less than 2/3 sigs */
            let shortcut = Syncedtx::to_sign(&sigfinale[0].txs); /* moving this to after the for loop made this code 3x faster. this is just a reminder to optimize everything later. (i can use [0]) */
            let m = vec![leader.clone(),shortcut.to_owned(),bincode::serialize(&Vec::<CompressedRistretto>::new()).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut s = Sha3_512::new();
            s.update(&m);
            let sigfinale = sigfinale.into_par_iter().filter(|x| Signature::verify(&x.leader, &mut s.clone(),&stkstate)).map(|x| x.to_owned()).collect::<Vec<NextBlock>>();
            /* my txt file codes are hella optimized but also hella incomplete with respect to polynomials and also hella disorganized */
            let sigs = sigfinale.par_iter().map(|x| x.leader.to_owned()).collect::<Vec<Signature>>();
            /* do i need to add an empty block option that requires >1/3 signatures of you as leader? */
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&sigs).unwrap().to_vec());
            let c = s.finalize();
            let m = vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(),last_name.clone(),c.to_vec(),bincode::serialize(&None::<([Signature;2],[Vec<u8>;2],u64)>).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut s = Sha3_512::new();
            s.update(&m);
            let leader = Signature::sign(&key, &mut s,&location);
            
            return NextBlock{emptyness: MultiSignature::default(), validators: sigs, shards: vec![], leader, txs, last_name: last_name.to_owned(), pools: vec![*pool], bnum: bnum.to_owned(), forker: None}
        }

        return NextBlock::default()
    }
    pub fn valimerge(key: &Scalar, location: &u64, leader: &CompressedRistretto, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, _pool_nums: &Vec<u16>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Signature {
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
                    blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>());
                }
            }
        }
        let s = blk.shards.clone();
        blk.shards = blk.shards.par_iter().enumerate().filter_map(|(i,x)| if s[..i].par_iter().all(|y| stkstate[*x as usize].0 != stkstate[*y as usize].0) {Some(x.to_owned())} else {None}).collect::<Vec<u64>>();
        


        let m = vec![leader.to_bytes().to_vec(),Syncedtx::to_sign(&blk.txs),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec(), last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        Signature::sign(&key,&mut s, &location)
    }
    pub fn finishmerge(key: &Scalar, location: &u64, sigs: &Vec<Signature>, blks: &Vec<NextBlock>, val_pools: &Vec<Vec<u64>>, pool0: &Vec<u64>, pool_nums: &Vec<u16>, bnum: &u64, last_name: &Vec<u8>, stkstate: &Vec<(CompressedRistretto,u64)>) -> NextBlock {
        let pool0 = pool0.into_par_iter().map(|x|stkstate[*x as usize].0).collect::<Vec<CompressedRistretto>>();
        let mut blks: Vec<NextBlock> = blks.par_iter().zip(val_pools).filter_map(|(x,y)| if x.verify(&y,&stkstate).is_ok() {Some(x.to_owned())} else {None}).collect();
        let mut blk = blks.remove(0);
        blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>());
        let mut tags = blk.txs.par_iter().map(|x| x.tags.clone()).flatten().collect::<HashSet<Tag>>();
        let mut pools = vec![0u16];
        for (mut b,p) in blks.into_iter().zip(pool_nums) {
            b.txs = b.txs.into_par_iter().filter(|t| {
                t.tags.par_iter().all(|x| !tags.contains(x))
            }).collect::<Vec<PolynomialTransaction>>();
            blk.txs.par_extend(b.txs.clone());
            let x = b.txs.len();
            tags = tags.union(&b.txs.into_par_iter().map(|x| x.tags).flatten().collect::<HashSet<Tag>>()).map(|&x| x).collect::<HashSet<CompressedRistretto>>();
            if x > 64 {
                pools.push(*p);
                blk.shards.par_extend(b.shards);
                blk.shards.par_extend(blk.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>());
            }
        }
        let s = blk.shards.clone();
        blk.shards = blk.shards.into_par_iter().enumerate().filter(|(i,x)| s[..*i].par_iter().all(|y| stkstate[*x as usize].0 != stkstate[*y as usize].0)).map(|(_,x)|x).collect::<Vec<u64>>();
        
        let leader = (key*PEDERSEN_H()).compress().as_bytes().to_vec();
        let m = vec![leader.clone(),Syncedtx::to_sign(&blk.txs),bincode::serialize(&blk.shards).unwrap(),bnum.to_le_bytes().to_vec(),last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        let sigs = sigs.into_par_iter().filter(|x|
            Signature::verify(x, &mut s.clone(), stkstate)
        ).collect::<Vec<&Signature>>();
        let sigs = sigs.into_par_iter().filter(|x| !pool0.clone().into_par_iter().all(|y| stkstate[x.pk as usize].0 != y)).collect::<Vec<&Signature>>();
        let sigcopy = sigs.clone();
        let sigs = sigs.into_par_iter().enumerate().filter_map(|(i,x)| if sigcopy[..i].par_iter().all(|y| x.pk != y.pk) {Some(x.to_owned())} else {None}).collect::<Vec<Signature>>();
        let mut s = Sha3_512::new();
        s.update(&bincode::serialize(&sigs).unwrap().to_vec());
        let c = s.finalize();

        let m = vec![BLOCK_KEYWORD.to_vec(),bnum.to_le_bytes().to_vec(), last_name.clone(),c.to_vec(),bincode::serialize(&blk.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
        let mut s = Sha3_512::new();
        s.update(&m);
        let leader = Signature::sign(&key, &mut s, &location);
        NextBlock{emptyness: MultiSignature::default(), validators: sigs, shards: blk.shards, leader, txs: blk.txs, last_name: last_name.clone(), pools, bnum: bnum.to_owned(), forker: None}
    }
    pub fn verify(&self, validator_pool: &Vec<u64>, stkstate: &Vec<(CompressedRistretto,u64)>) -> Result<bool, &'static str> {
        if let Some((s,v,b)) = &self.forker {
            if s[0].pk != s[1].pk { /* leader could cause a fork by messing with fork section or which members sign */
                return Err("forker is not 1 person")
            }
            if (s[0].c == s[1].c) & (s[0].r == s[1].r) {
                return Err("forker is not a forker")
            }
            let x = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[0].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            let y = vec![BLOCK_KEYWORD.to_vec(),b.to_le_bytes().to_vec(),v[1].to_owned()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut h = Sha3_512::new();
            h.update(&x);
            let mut a = Sha3_512::new();
            a.update(&y);
            if !s[0].verify(&mut h, &stkstate) | !s[1].verify(&mut a, &stkstate) {
                return Err("the forker was framed")
            }
        }
        if self.validators.len() > 0 {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
            let c = s.finalize();
            let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec(),bincode::serialize(&self.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut h = Sha3_512::new();
            h.update(&m);
            if !self.leader.verify(&mut h, &stkstate) {
                return Err("leader is fake")
            }
            let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(),Syncedtx::to_sign(&self.txs),bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut h = Sha3_512::new();
            h.update(&m);
            if !self.validators.par_iter().all(|x| x.verify(&mut h.clone(), &stkstate)) {
                return Err("at least 1 validator is fake")
            }
            if !self.validators.par_iter().all(|x| !validator_pool.clone().into_par_iter().all(|y| x.pk != y)) {
                return Err("at least 1 validator is not in the pool")
            }
            if validator_pool.par_iter().filter(|x| !self.validators.par_iter().all(|y| x.to_owned() != &y.pk)).count() < (2*NUMBER_OF_VALIDATORS as usize/3) {
                return Err("there aren't enough validators")
            }
            let x = self.validators.par_iter().map(|x| x.pk).collect::<Vec<u64>>();
            if !x.clone().par_iter().enumerate().all(|(i,y)| x.clone()[..i].into_par_iter().all(|z| y != z)) {
                return Err("there's multiple signatures from the same validator")
            }
        } else {
            if self.txs.len() > 0 {
                return Err("the block isn't empty!")
            }
            let who = validator_pool.into_par_iter().filter_map(|x|
                if self.emptyness.pk.par_iter().all(|y| y!=x) {
                    Some(stkstate[*x as usize].0)
                } else { // may need to add more checks here
                    None
                }
            ).collect::<Vec<_>>();
            if who.len() <= NUMBER_OF_VALIDATORS as usize/2 {
                return Err("there's not enough validators for the empty block")
            }
            let mut m = stkstate[self.leader.pk as usize].0.as_bytes().to_vec();
            m.extend(&self.last_name);
            if !MultiSignature::verify_group(&self.emptyness.y,&self.emptyness.x,&m,&who) {
                return Err("there's a problem with the multisignature")
            }
            let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(),self.last_name.clone(),bincode::serialize(&self.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut s = Sha3_512::new();
            s.update(&m);
            if !self.leader.verify(&mut s,&stkstate) {
                return Err("there's a problem with the leader's multisignature group signature")
            }
        }
        return Ok(true)
    }
    pub fn scan_as_noone(&self, valinfo: &mut Vec<(CompressedRistretto,u64)>,val_pools: &Vec<Vec<u64>>, queue: &mut Vec<VecDeque<usize>>, exitqueue: &mut Vec<VecDeque<usize>>, comittee: &mut Vec<Vec<usize>>, save_history: bool) {
        let mut val_pools = val_pools.into_par_iter().enumerate().filter_map(|x|if !self.pools.par_iter().all(|y|*y!=(x.0 as u16)) {Some(x.1.clone())} else {None}).collect::<Vec<Vec<u64>>>();
        
        
        let mut info = Syncedtx::from(&self.txs);
        if save_history {History::append(&info.txout)};
        let fees = self.txs.par_iter().map(|x|x.fee).sum::<u64>();
        let inflation = INFLATION_CONSTANT/2u64.pow(self.bnum as u32/INFLATION_EXPONENT);
        
        if self.txs.len() > 0 {
            let profits = fees/(self.validators.len() as u64);
            for v in val_pools.remove(0) {
                if self.validators.par_iter().all(|x|x.pk!=v) {
                    valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
                }
                else {
                    valinfo[v as usize].1 += profits+inflation;
                }
            }
            for vv in val_pools {
                for v in vv {
                    if self.shards.par_iter().all(|x|*x!=v) {
                        valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
                    } else {
                        valinfo[v as usize].1 += profits+inflation;
                    }
                }
            }
        } else {
            for v in val_pools.remove(0) {
                if self.emptyness.pk.par_iter().all(|x| *x != v) {
                    valinfo[v as usize].1 += inflation;
                } else {
                    valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
                }
            }
        }
        for x in info.stkout.iter().rev() {
            valinfo.remove(*x as usize);
            *queue = queue.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<VecDeque<_>>();
                if z.len() == 0 {
                    VecDeque::from_iter([0usize])
                } else {
                    z
                }
            }).collect::<Vec<_>>();
            *exitqueue = exitqueue.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<VecDeque<_>>();
                if z.len() == 0 {
                    VecDeque::from_iter([0usize])
                }
                else {
                    z
                }
            }).collect::<Vec<_>>();
            *comittee = comittee.into_par_iter().map(|y| {
                let z = y.into_par_iter().filter_map(|z|
                    if *z > *x as usize {Some(*z - 1)}
                    else if *z == *x as usize {None}
                    else {Some(*z)}
                ).collect::<Vec<_>>();
                if z.len() == 0 {
                    vec![0usize]
                }
                else {
                    z
                }
            }).collect::<Vec<_>>();
        }
        queue.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..NUMBER_OF_VALIDATORS as usize-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<VecDeque<usize>>();
            x.append(&mut y);
        });
        exitqueue.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..NUMBER_OF_VALIDATORS as usize-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<VecDeque<usize>>();
            x.append(&mut y);
        });
        comittee.par_iter_mut().for_each(|x| {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&x).unwrap());
            let mut v = Scalar::from_hash(s.clone()).as_bytes().to_vec();
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            s.update(&bincode::serialize(&x).unwrap());
            v.append(&mut Scalar::from_hash(s.clone()).as_bytes().to_vec());
            let mut y = (0..NUMBER_OF_VALIDATORS as usize-x.len()).map(|i| x[v[i] as usize%x.len()]).collect::<Vec<usize>>();
            x.append(&mut y);
        });


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
        if !newtags.par_iter().all(|x| alltagsever.par_iter().all(|y|y!=x)) {
            println!("you got burnt (someone sent you faerie gold!)"); // i want this in a seperate function
        }
        alltagsever.par_extend(&newtags);

        *mine = mine.into_par_iter().filter_map(|(j,a)| if x.tags.par_iter().all(|x| x != &a.tag.unwrap()) {Some((*j,a.clone()))} else {None} ).collect::<Vec<(u64,OTAccount)>>();
        *height += x.txout.len() as u64;
        mine.par_extend(newmine);

        x
    }
    pub fn scanstk(&self, me: &Account, mine: &mut Vec<[u64;2]>, height: &mut u64, val_pools: &Vec<u64>) {
        let stkin = self.txs.par_iter().map(|x|
            x.outputs.par_iter().filter_map(|y| 
                if let Ok(z) = stakereader_acc().read_ot(y) {Some(z)} else {None}
            ).collect::<Vec<_>>()
        ).flatten().collect::<Vec<OTAccount>>();
        mine.par_extend(stkin.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.stake_acc().receive_ot(x) {Some([i as u64+*height,u64::from_le_bytes(y.com.amount.unwrap().as_bytes()[..8].try_into().unwrap())])} else {None}).collect::<Vec<[u64;2]>>());

        // println!("-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:\n{:#?}",stkin);
        // println!("-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:\n{:#?}",stkin.len());
        // println!("-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:\n{:?}",mine);

        if self.emptyness.y == Scalar::from(0u8) {
            let fees = self.txs.par_iter().map(|x|x.fee).sum::<u64>();
            let profits = fees/(self.validators.len() as u64);
            let inflation = INFLATION_CONSTANT/2u64.pow(self.bnum as u32/INFLATION_EXPONENT); // todo: MAKE FLOATING POINT TO AVOID HARD CUTOFFS
            // println!("{:?}",val_pools);
            for &v in val_pools {
                for (i,m) in mine.clone().iter().enumerate() {
                    if m[0] == v {
                        if self.validators.par_iter().all(|x|x.pk!=v) { // NEEED TO SUBTRACT LOCATION WHEN STAKERS LEAVE
                            let delta = m[1]/1000;
                            mine[i][1] -= delta;
                        } else {
                            // println!("{:?}",mine[i].1.com.amount);
                            let delta = profits+inflation;
                            mine[i][1] += delta;
                        }
                    }
                }
            }
            // println!("-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:\n{:?}",mine);
    
    
            let stkout = self.txs.par_iter().filter_map(|x|
                if x.inputs.len() == 8 {Some(u64::from_le_bytes(x.inputs.to_owned().try_into().unwrap()))} else {None}
            ).collect::<Vec<u64>>();
    
    
    
    
            for (i,m) in mine.clone().iter().enumerate().rev() {
                for v in &stkout {
                    if m[0] == *v {
                        mine.remove(i as usize);
                    }
                }
                for n in stkout.iter() {
                    if *n < m[0] {
                        mine[i][0] -= 1;
                    }
                    else {
                        break;
                    }
                }
            }
            // println!("-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:-:\n{:?}",mine);
            *height += stkin.len() as u64;
            *height -= stkout.len() as u64;
        } else {
            let inflation = INFLATION_CONSTANT/2u64.pow(self.bnum as u32/INFLATION_EXPONENT); // todo: MAKE FLOATING POINT TO AVOID HARD CUTOFFS
            for &v in val_pools {
                for (i,m) in mine.clone().iter().enumerate() {
                    if m[0] == v {
                        if !self.emptyness.pk.par_iter().all(|x|*x!=v) { // NEEED TO SUBTRACT LOCATION WHEN STAKERS LEAVE
                            let delta = m[1]/1000;
                            mine[i][1] -= delta;
                        }
                        else {
                            let delta = inflation;
                            mine[i][1] += delta;
                        }
                    }
                }
            }

        }
    }
    pub fn update_bloom(&self,bloom:&BloomFile) {
        self.txs.par_iter().map(|x| x.tags.par_iter().map(|y| bloom.insert(y.as_bytes())).collect::<Vec<_>>()).collect::<Vec<_>>();
    }
    pub fn tolightning(&self) -> LightningSyncBlock {
        LightningSyncBlock {
            emptyness: self.emptyness.to_owned(),
            validators: self.validators.to_owned(),
            shards: self.shards.to_owned(),
            leader: self.leader.to_owned(),
            info: Syncedtx::from(&self.txs),
            pools: self.pools.to_owned(),
            bnum: self.bnum.to_owned(),
            last_name: self.last_name.to_owned(),
            forker: self.forker.to_owned(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct LightningSyncBlock {
    pub emptyness: MultiSignature,
    pub validators: Vec<Signature>,
    pub shards: Vec<u64>,
    pub leader: Signature,
    pub info: Syncedtx,
    pub pools: Vec<u16>,
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
            let mut h = Sha3_512::new();
            h.update(&x);
            let mut a = Sha3_512::new();
            a.update(&y);
            if !s[0].verify(&mut h, &stkstate) | !s[1].verify(&mut a, &stkstate) {
                return Err("the forker was framed")
            }
        }
        if self.validators.len() > 0 {
            let mut s = Sha3_512::new();
            s.update(&bincode::serialize(&self.validators).unwrap().to_vec());
            let c = s.finalize();
            let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone(),c.to_vec(),bincode::serialize(&self.forker).unwrap()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut h = Sha3_512::new();
            h.update(&m);
            if !self.leader.verify(&mut h, &stkstate) {
                return Err("leader is fake")
            }
            let m = vec![stkstate[self.leader.pk as usize].0.as_bytes().to_vec().clone(),bincode::serialize(&self.info).unwrap(),bincode::serialize(&self.shards).unwrap(),self.bnum.to_le_bytes().to_vec(), self.last_name.clone()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut h = Sha3_512::new();
            h.update(&m);
            if !self.validators.par_iter().all(|x| x.verify(&mut h.clone(), &stkstate)) {
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
        } else {
            if self.info.tags.len() != 0 {
                return Err("the block isn't empty!")
            }
            let who = validator_pool.into_par_iter().filter_map(|x|
                if self.emptyness.pk.par_iter().all(|y| y!=x) {
                    Some(stkstate[*x as usize].0)
                } else { // may need to add more checks here
                    None
                }
            ).collect::<Vec<_>>();
            if who.len() <= NUMBER_OF_VALIDATORS as usize/2 {
                return Err("there's not enough validators for the empty block")
            }
            let mut m = stkstate[self.leader.pk as usize].0.as_bytes().to_vec();
            m.extend(&self.last_name);
            if !MultiSignature::verify_group(&self.emptyness.y,&self.emptyness.x,&m,&who) {
                return Err("there's a problem with the multisignature")
            }
            let m = vec![BLOCK_KEYWORD.to_vec(),self.bnum.to_le_bytes().to_vec(),self.last_name.clone(),bincode::serialize(&self.emptyness).unwrap().to_vec()].into_par_iter().flatten().collect::<Vec<u8>>();
            let mut s = Sha3_512::new();
            s.update(&m);
            if !self.leader.verify(&mut s,&stkstate) {
                return Err("there's a problem with the leader's multisignature group signature")
            }
        }
        return Ok(true)
    }
    pub fn scan(&self, me: &Account, mine: &mut Vec<(u64,OTAccount)>, height: &mut u64, alltagsever: &mut Vec<CompressedRistretto>) -> Syncedtx {
        let x = self.info.clone();
        let newmine = x.txout.par_iter().enumerate().filter_map(|(i,x)| if let Ok(y) = me.receive_ot(x) {Some((i as u64+*height,y))} else {None}).collect::<Vec<(u64,OTAccount)>>();
        let newtags = newmine.par_iter().map(|x|x.1.tag.unwrap()).collect::<Vec<CompressedRistretto>>();
        if !newtags.par_iter().all(|x| alltagsever.par_iter().all(|y|y!=x)) {
            println!("you got burnt (someone sent you faerie gold!)"); // i want this in a seperate function
        }
        alltagsever.par_extend(&newtags);

        *mine = mine.into_par_iter().filter_map(|(j,a)| if x.tags.par_iter().all(|x| x != &a.tag.unwrap()) {Some((*j,a.clone()))} else {None} ).collect::<Vec<(u64,OTAccount)>>();
        *height += x.txout.len() as u64;
        mine.par_extend(newmine);

        x
    }
    pub fn scan_as_noone(&self,valinfo: &mut Vec<(CompressedRistretto,u64)>,val_pools: &Vec<Vec<u64>>) {
        let mut val_pools = val_pools.into_par_iter().enumerate().filter_map(|x|if self.pools.par_iter().all(|y|*y!=(x.0 as u16)) {None} else {Some(x.1.clone())}).collect::<Vec<Vec<u64>>>();

        let mut info =self.info.clone();
        let fees = info.fees;
        let profits = fees/(self.validators.len() as u64);
        let inflation = INFLATION_CONSTANT/2u64.pow(self.bnum as u32/INFLATION_EXPONENT);
        for v in val_pools.remove(0) {
            if self.validators.par_iter().all(|x|x.pk!=v) {
                valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
            }
            else {
                valinfo[v as usize].1 += profits+inflation;
            }
        }
        for vv in val_pools {
            for v in vv {
                if self.shards.par_iter().all(|x|*x!=v) {
                    valinfo[v as usize].1 -= valinfo[v as usize].1/1000;
                }
                else {
                    valinfo[v as usize].1 += profits+inflation;
                }
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




pub fn select_stakers(block: &Vec<u8>, shard: &u128, queue: &mut VecDeque<usize>, exitqueue: &mut VecDeque<usize>, comittee: &mut Vec<usize>, stkstate: &Vec<(CompressedRistretto,u64)>) {
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
    let mut loser = (0..REPLACERATE).collect::<Vec<usize>>().par_iter().map(|x| {
        let mut s = s.clone();
        s.write(&x.to_le_bytes()[..]);
        let c = s.finish() as usize;
        c%NUMBER_OF_VALIDATORS as usize
    }).collect::<VecDeque<usize>>();
    // println!("loser locations: {:?}",loser);
    exitqueue.append(&mut loser);
    let loser = exitqueue.par_drain(..REPLACERATE).collect::<Vec<usize>>();

    for (i,j) in loser.iter().enumerate() {
        comittee[*j] = winner[i];
    }
}








pub struct History {}

static FILE_NAME: &str = "history";

impl History {
    pub fn initialize() {
        File::create(FILE_NAME).unwrap();
    }
    pub fn get(location: &u64) -> [CompressedRistretto;2] {
        let mut byte = [0u8;64];
        let mut r = BufReader::new(File::open(FILE_NAME).unwrap());
        r.seek(SeekFrom::Start(location*64)).expect("Seek failed");
        r.read(&mut byte).unwrap();
        [CompressedRistretto::from_slice(&byte[..32]),CompressedRistretto::from_slice(&byte[32..])] // OTAccount::summon_ota() from there
    }
    pub fn get_raw(location: &u64) -> [u8; 64] {
        let mut bytes = [0u8;64];
        let mut r = BufReader::new(File::open(FILE_NAME).unwrap());
        r.seek(SeekFrom::Start(location*64)).expect("Seek failed");
        r.read(&mut bytes).unwrap();
        bytes
    }
    pub fn read_raw(bytes: &Vec<u8>) -> OTAccount { // assumes the bytes start at the beginning
        OTAccount::summon_ota(&[CompressedRistretto::from_slice(&bytes[..32]),CompressedRistretto::from_slice(&bytes[32..64])]) // OTAccount::summon_ota() from there
    }
    pub fn append(accs: &Vec<OTAccount>) {
        let buf = accs.into_par_iter().map(|x| [x.pk.compress().as_bytes().to_owned(),x.com.com.compress().as_bytes().to_owned()].to_owned()).flatten().flatten().collect::<Vec<u8>>();
        let mut f = OpenOptions::new().append(true).open(FILE_NAME).unwrap();
        f.write_all(&buf.par_iter().map(|x|*x).collect::<Vec<u8>>()).unwrap();

    }
}



pub struct StakerState {}

static STAKE_FILE_NAME: &str = "stkstate";

impl StakerState {
    pub fn initialize() {
        File::create(STAKE_FILE_NAME).unwrap();
    }
    pub fn read() -> Vec<(CompressedRistretto,u64)> {
        let mut bytevec = Vec::<u8>::new();
        let mut r = File::open(STAKE_FILE_NAME).unwrap();
        r.read_to_end(&mut bytevec).unwrap();
        // bytevec.par_chunks(40).map(|x| (CompressedRistretto::from_slice(&x[..32]),u64::from_le_bytes(x[32..].try_into().unwrap()))).collect::<Vec<(CompressedRistretto,u64)>>()
        bincode::deserialize(&bytevec[..]).unwrap()
    }
    pub fn replace(valinfo: &Vec<(CompressedRistretto,u64)>) {
        // let bytevec = valinfo.into_par_iter().flat_map(|(x,y)|{
        //     let a = x.to_bytes().to_vec(); let b = y.to_le_bytes().to_vec();
        //     a.into_par_iter().chain(b.into_par_iter()).collect::<Vec<u8>>()
        // }).collect::<Vec<u8>>();
        let bytevec = bincode::serialize(valinfo).unwrap();
        
        let mut f = File::create("throwaway").unwrap();
        f.write_all(&bytevec).unwrap();
        rename("throwaway",STAKE_FILE_NAME).unwrap();
    }
}





#[cfg(test)]
mod tests {

    #[test]
    fn multisignature_test() {
        use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
        use curve25519_dalek::scalar::Scalar;
        use crate::validation::MultiSignature;

        let message = "hi!!!".as_bytes().to_vec();
        let sk = (0..100).into_iter().map(|_| Scalar::from(rand::random::<u64>())).collect::<Vec<_>>();
        let pk = sk.iter().map(|x| x*RISTRETTO_BASEPOINT_POINT).collect::<Vec<_>>();

        let x = sk.iter().map(|k| MultiSignature::gen_group_x(&k,&0u64).decompress().unwrap()).collect::<Vec<_>>();
        let xt = MultiSignature::sum_group_x(&x);
        let y = sk.iter().map(|k| MultiSignature::try_get_y(&k, &0u64, &message, &xt)).collect::<Vec<_>>();
        let yt = MultiSignature::sum_group_y(&y);



        assert!(MultiSignature::verify_group(&yt, &xt, &message, &pk.iter().map(|x| x.compress()).collect::<Vec<_>>()));
        assert!(MultiSignature::verify_group(&yt, &xt, &message, &pk.iter().rev().map(|x| x.compress()).collect::<Vec<_>>()));
        assert!(!MultiSignature::verify_group(&yt, &xt, &message, &pk[..99].iter().map(|x| x.compress()).collect::<Vec<_>>()));
        assert!(!MultiSignature::verify_group(&yt, &xt, &"bye!!!".as_bytes().to_vec(), &pk.iter().map(|x| x.compress()).collect::<Vec<_>>()));
    }
}