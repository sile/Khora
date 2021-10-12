//! Implementation of a bloom filter in rust
//
//! # Basic Usage
//!
//! ```rust,no_run
//! use bloom::BloomFilter;
//! let expected_num_items = 1000;
//! let false_positive_rate = 0.01;
//! let mut filter:BloomFilter = BloomFilter::with_rate(false_positive_rate,expected_num_items);
//! filter.insert(&1);
//! filter.contains(&1); /* true */
//! filter.contains(&2); /* false */
//! ```
//!
//! # False Positive Rate
//! The false positive rate is specified as a float in the range
//! (0,1).  If indicates that out of `X` probes, `X * rate` should
//! return a false positive.  Higher values will lead to smaller (but
//! more inaccurate) filters.
//!

#![cfg_attr(feature = "do-bench", feature(test))]

extern crate core;
extern crate bit_vec;
extern crate rand;

use bit_vec::BitVec;
use rand::Rng;
use std::cmp::{min,max};
use std::hash::Hasher;
use std::iter::Iterator;
use std::fs;
use ahash::AHasher;
use std::fs::OpenOptions;
use std::fs::File;
use std::io::{Seek, SeekFrom, Read, Write, BufReader};

#[derive(Clone)]

struct HashIter {
    h: AHasher,
    i: u32,
    count: u32,
}

impl Iterator for HashIter {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        if self.i == self.count {
            return None;
        }
        self.h.write(&self.i.to_le_bytes().to_vec());
        self.i+=1;
        Some(self.h.clone().finish())
    }
}



#[derive(Clone)]
pub struct BloomFile {
    h: AHasher,
    key1: u128,
    key2: u128,
}

static FILE_NAME: &str = "bloomfile";
// const FILE_SIZE: usize = 1_000_000*4;// 1_000_000_000*4; // problems are somewhere
// const HASHES: u32 = 13;
const FILE_SIZE: u64 = 4_000_000; // for tests (in bits)
const HASHES: u32 = 6; // for tests

impl BloomFile {

    pub fn initialize_bloom_file() { // lol this is actually fast!
        // 6 hashes is best for 1_000_000_000 outputs
        // fs::remove_file(FILE_NAME).unwrap(); // this is just for testing in reality this wouldn't be unwrapped because the file may not exist
        let mut f = File::create(FILE_NAME).unwrap();
        f.write(&[0b00000000u8;FILE_SIZE as usize/8]).unwrap();
        // let mut f = OpenOptions::new().append(true).open(FILE_NAME).unwrap();
        // f.write_all(&[0u8;FILE_SIZE as usize]).unwrap();
        // for _ in 0..100 {
        //     f.write_all(&[0u8;FILE_SIZE/100]).unwrap();
        // }
    }

    pub fn from_keys(key1: u128, key2: u128) -> BloomFile {
        BloomFile {
            h: AHasher::new_with_keys(key1,key2),
            key1,
            key2,
        }

    }
    pub fn get_keys(&self) -> [u128;2] {
        [self.key1, self.key2]
    }
    /// Insert item into this bloomfilter
    pub fn insert(&self, item: &[u8;32]) { // loc, pk, com = 32*3 = 96
        for h in self.get_hashes(item) {
            let h = h % FILE_SIZE;

            let mut byte = [0u8];
            let mut f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(false)
                .open(FILE_NAME)
                .unwrap();
            f.seek(SeekFrom::Start(h/8)).expect("Seek failed");

            f.read(&mut byte).unwrap();
            let mut delta = 0b00000001u8;
            delta <<= h%8;
            // println!("{:#8b} |\n{:#8b} =",byte[0],delta);
            byte[0] |= delta;
            // println!("{:#8b}",byte[0]);
            assert!(byte[0] & delta != 0b00000000u8);
            f.seek(SeekFrom::Start(h/8)).expect("Seek failed");
            f.write(&byte).expect("Unable to write data");

            
        }
    }
    pub fn contains(&self, item: &[u8;32]) -> bool {
        for h in self.get_hashes(item) {
            let h = h % FILE_SIZE;
            let mut byte = [0u8];
            let mut r = OpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open(FILE_NAME)
                .unwrap();
            r.seek(SeekFrom::Start(h/8)).expect("Seek failed");

            r.read(&mut byte).expect("Unable to read data");
            let mut delta = 0b00000001u8;
            delta <<= h%8;
            if (byte[0] & delta) == 0b00000000u8 {
                return false;
            }
        }
        true
    }

    fn get_hashes(&self, item: &[u8;32]) -> HashIter {
        let mut h = self.h.clone();
        h.write(item);
        HashIter {
            h,
            i: 0,
            count: HASHES, // 4 is best for ~1_283_000_000 outputs (1 in 20 wrong)
        }
    }
}
















/// Return the optimal number of hashes to use for the given number of
/// bits and items in a filter
pub fn optimal_num_hashes(num_bits: usize, num_items: u32) -> u32 {
    min(
        max(
            (num_bits as f32 / num_items as f32 * core::f32::consts::LN_2).round() as u32,
             2
           ),
        200
      )
}

/// Return the number of bits needed to satisfy the specified false
/// positive rate, if the filter will hold `num_items` items.
pub fn needed_bits(false_pos_rate:f32, num_items: u32) -> usize {
    let ln22 = core::f32::consts::LN_2 * core::f32::consts::LN_2;
    (num_items as f32 * ((1.0/false_pos_rate).ln() / ln22)).round() as usize
}

// #[cfg(feature = "do-bench")]
// #[cfg(test)]
// mod bench {
//     extern crate test;
//     use self::test::Bencher;
//     use rand::{self,Rng};

//     use super::BloomFilter;

//     #[bench]
//     fn insert_benchmark(b: &mut Bencher) {
//         let cnt = 500000;
//         let rate = 0.01 as f32;

//         let mut bf:BloomFilter = BloomFilter::with_rate(rate,cnt);
//         let mut rng = rand::thread_rng();

//         b.iter(|| {
//             let mut i = 0;
//             while i < cnt {
//                 let v = rng.gen::<i32>();
//                 bf.insert(&v);
//                 i+=1;
//             }
//         })
//     }

//     #[bench]
//     fn contains_benchmark(b: &mut Bencher) {
//         let cnt = 500000;
//         let rate = 0.01 as f32;

//         let mut bf:BloomFilter = BloomFilter::with_rate(rate,cnt);
//         let mut rng = rand::thread_rng();

//         let mut i = 0;
//         while i < cnt {
//             let v = rng.gen::<i32>();
//             bf.insert(&v);
//             i+=1;
//         }

//         b.iter(|| {
//             i = 0;
//             while i < cnt {
//                 let v = rng.gen::<i32>();
//                 bf.contains(&v);
//                 i+=1;
//             }
//         })
//     }
// }

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use curve25519_dalek::scalar::Scalar;
    use rand::{self,Rng};
    use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
    use super::BloomFile;

    #[test]
    fn simple() {
        BloomFile::initialize_bloom_file();
        let b: BloomFile = BloomFile::from_keys(0,0);
        b.insert(&Scalar::from(1u8).as_bytes());
        assert!(b.contains(&Scalar::from(1u8).as_bytes()));
        assert!(!b.contains(&Scalar::from(2u8).as_bytes()));
    }

    #[test]
    fn bloom_test() {
        let cnt = 500_000;
        let bits = 4_000_000;
        let hashes = 6; // the problem disapears when there's 1 hash..., problem starts at like 3ish
        let rate = 0.021577141 as f32;

        BloomFile::initialize_bloom_file();
        let b: BloomFile = BloomFile::from_keys(1,2);
        let mut set: HashSet<[u8;32]> = HashSet::new();
        let mut rng = rand::thread_rng();

        let mut i = 0;
        while i < cnt {
            // let v = rng.gen::<u32>();
            let v = i as u32;
            assert!(set.insert(*Scalar::from(v).as_bytes()));
            b.insert(&Scalar::from(v).as_bytes());
            i+=1;
        }

        i = 0;
        let mut false_positives = 0;
        let mut true_positives = 0;
        let mut true_negatives = 0;
        while i < cnt {
            let v = rng.gen::<u32>();
            // let v = i as u32;
            match (b.contains(&Scalar::from(v).as_bytes()),set.contains(Scalar::from(v).as_bytes())) {
                (true, false) => { false_positives += 1; }
                (false, true) => { assert!(false); } // should never happen
                (true, true) => { true_positives += 1; }
                (false, false) => { true_negatives += 1; }
            }
            i+=1;
        }

        // make sure we're not too far off
        let actual_rate = false_positives as f32 / (false_positives + true_negatives) as f32;
        println!("fp: {}    tn: {}  tp: {}",false_positives,true_negatives,true_positives);
        println!("expected: {}",rate);
        println!("actual:   {}",actual_rate);
        assert!(actual_rate > (rate-0.001));
        assert!(actual_rate < (rate+0.001));
    }

    #[test]
    fn bloom_parallell() {
        let cnt = 500_000 as usize;
        let bits = 4_000_000;
        let hashes = 6; // the problem disapears when there's 1 hash..., problem starts at like 3ish
        let rate = 0.021577141 as f32;

        BloomFile::initialize_bloom_file();
        let b: BloomFile = BloomFile::from_keys(1,2);
        let mut set: HashSet<[u8;32]> = HashSet::new();


        let mut i = 0;
        while i < cnt {
            // let v = rng.gen::<u32>();
            let v = i as u32;
            assert!(set.insert(*Scalar::from(v).as_bytes()));
            b.insert(&Scalar::from(v).as_bytes());
            i+=1;
        }

        let res = (0..2*cnt as u32).into_par_iter().map(|v| {
            // let mut rng = rand::thread_rng();
            // let v = rng.gen::<u32>();
            match (b.contains(&Scalar::from(v).as_bytes()),set.contains(Scalar::from(v).as_bytes())) {
                (true, false) => { return 0 }
                (false, true) => { assert!(false); return -1 } // should never happen
                (true, true) => { return 1 }
                (false, false) => { return 2 }
            }
        }).collect::<Vec<_>>();
        let false_positives = res.par_iter().filter(|&&x| x == 0).count();
        let true_positives = res.par_iter().filter(|&&x| x == 1).count();
        let true_negatives = res.par_iter().filter(|&&x| x == 2).count();

        // make sure we're not too far off
        let actual_rate = false_positives as f32 / (false_positives + true_negatives) as f32;
        println!("fp: {}    tn: {}  tp: {}",false_positives,true_negatives,true_positives);
        println!("expected: {}",rate);
        println!("actual:   {}",actual_rate);
        assert!(actual_rate > (rate-0.001));
        assert!(actual_rate < (rate+0.001));
        assert!(true_positives == cnt);
    }


    #[test]
    fn bloom_parallell_writing() {
        let cnt = 500_000 as usize;
        let bits = 4_000_000;
        let hashes = 6; // the problem disapears when there's 1 hash..., problem starts at like 3ish
        let rate = 0.021577141 as f32;

        BloomFile::initialize_bloom_file();
        let b: BloomFile = BloomFile::from_keys(1,2);
        let mut set: HashSet<[u8;32]> = HashSet::new();


        (0..cnt as u32).into_iter().for_each(|v| {
            set.insert(*Scalar::from(v).as_bytes());
        });

        (0..cnt as u32).into_par_iter().for_each(|v| {
            b.insert(&Scalar::from(v).as_bytes());
        });
        

        let res = (0..2*cnt as u32).into_par_iter().map(|v| {
            // let mut rng = rand::thread_rng();
            // let v = rng.gen::<u32>();
            match (b.contains(&Scalar::from(v).as_bytes()),set.contains(Scalar::from(v).as_bytes())) {
                (true, false) => { return 0 }
                (false, true) => { return -1 } // it may be allowed to have false negatives because the chance that 2 items in different threads try to write in the same place is (1 - HASHES/FILE_SIZE)*(1 - HASHES/(FILE_SIZE-1))...*(1 - HASHES/(FILE_SIZE-CORES+1))
                (true, true) => { return 1 } // ~= (1 - HASHES/FILE_SIZE)^(CORES - 1) > 0.999 on most computers for our uses
                (false, false) => { return 2 } // which is fine because there's 128 validators that need to agree and their errors are all in different places
            }
        }).collect::<Vec<_>>();
        let false_positives = res.par_iter().filter(|&&x| x == 0).count();
        let false_negatives = res.par_iter().filter(|&&x| x == -1).count();
        let true_positives = res.par_iter().filter(|&&x| x == 1).count();
        let true_negatives = res.par_iter().filter(|&&x| x == 2).count();

        // make sure we're not too far off
        let actual_rate = false_positives as f32 / (false_positives + true_negatives) as f32;
        let error_rate = false_negatives as f32 / (false_negatives + true_positives) as f32;
        println!("fp: {}    tn: {}  tp: {}  fn: {}",false_positives,true_negatives,true_positives,false_negatives);
        println!("expected: {}",rate);
        println!("actual:   {}",actual_rate);
        println!("error:    {}",error_rate);
        assert!(actual_rate > (rate-0.001));
        assert!(actual_rate < (rate+0.001));
        assert!(error_rate < 0.001);
    }
}
