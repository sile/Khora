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
use std::os::unix::prelude::FileExt;
use ahash::AHasher;
use std::fs::OpenOptions;
use std::fs::File;
use std::io::{Seek, SeekFrom, Read, Write, BufReader};

#[derive(Clone)]
// pub struct BloomFilter {
//     pub bits: BitVec,
//     pub num_hashes: u32,
//     h1: AHasher,
//     h2: AHasher,
// }

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

// impl BloomFilter {

//     pub fn initialize_bloom(key1: u128, key2: u128) -> BloomFilter {
//         // cap 1000 tags/block (THIS EFFECTS THE GENERATE POLYNOMIALS THING FOR MICROTRANSACTIONS)
//         // this should be fine for over 1,500,000,000 tags

//         BloomFilter { // there's memory issues with writing 32GB on my laptop
//             // bits: BitVec::from_elem(32_000_000_000,false),// 4 (maybe 8 later) GB
//             bits: BitVec::from_elem(32_000_000,false),// 4 (maybe 8 later) GB
//             num_hashes: 13,
//             h1: AHasher::new_with_keys(key1,0),
//             h2: AHasher::new_with_keys(key2,0),
//         }

//     }

//     pub fn from_file(fname: String, key1: u128, key2: u128) -> BloomFilter {
//         BloomFilter {
//             bits: BitVec::from_bytes(&fs::read(fname).unwrap()),
//             num_hashes: 13,
//             h1: AHasher::new_with_keys(key1, 0),
//             h2: AHasher::new_with_keys(key2, 0),
//         }
//     }

//     /// Create a new BloomFilter with the specified number of bits,
//     /// and hashes
//     pub fn with_size(num_bits: usize, num_hashes: u32) -> BloomFilter {
//         let mut rng = rand::thread_rng();
//         BloomFilter {
//             bits: BitVec::from_elem(num_bits,false),
//             num_hashes: num_hashes,
//             h1: AHasher::new_with_keys(rng.gen::<u128>(),rng.gen::<u128>()),
//             h2: AHasher::new_with_keys(rng.gen::<u128>(),rng.gen::<u128>()),
//         }
//     }

//     /// create a BloomFilter that expectes to hold
//     /// `expected_num_items`.  The filter will be sized to have a
//     /// false positive rate of the value specified in `rate`.
//     pub fn with_rate(rate: f32, expected_num_items: u32) -> BloomFilter {
//         let bits = needed_bits(rate,expected_num_items);
//         BloomFilter::with_size(bits,optimal_num_hashes(bits,expected_num_items))
//     }

//     /// Get the number of bits this BloomFilter is using
//     pub fn num_bits(&self) -> usize {
//         self.bits.len()
//     }

//     /// Get the number of hash functions this BloomFilter is using
//     pub fn num_hashes(&self) -> u32 {
//         self.num_hashes
//     }

//     /// Insert item into this bloomfilter
//     pub fn insert(& mut self,item: &[u8;32]) {
//         for h in self.get_hashes(item) {
//             let idx = (h % self.bits.len() as u64) as usize;
//             self.bits.set(idx,true)
//         }
//     }

//     /// Check if the item has been inserted into this bloom filter.
//     /// This function can return false positives, but not false
//     /// negatives.
//     pub fn contains(&self, item: &[u8;32]) -> bool {

//         // let mut thgs = vec![]; // that's a lot slower and most will be false
//         // for h in self.get_hashes(item) {
//         //     thgs.push(h);
//         // }
//         // thgs.par_iter().all(
//         //     |h| self.bits.get((h % self.bits.len() as u64) as usize).unwrap()
//         // )
//         for h in self.get_hashes(item) {
//             let idx = (h % self.bits.len() as u64) as usize;
//             match self.bits.get(idx) {
//                 Some(b) => {
//                     if !b {
//                         return false;
//                     }
//                 }
//                 None => { panic!("Hash mod failed"); }
//             }
//         }
//         true
//     }

//     /// Remove all values from this BloomFilter
//     pub fn clear(&mut self) {
//         self.bits.clear();
//     }


//     fn get_hashes(&self, item: &[u8;32]) -> HashIter {
//         let mut h1 = self.h1.clone();
//         let mut h2 = self.h2.clone();
//         h1.write(item);
//         h2.write(item);
//         let h1 = h1.finish();
//         let h2 = h2.finish();
//         HashIter {
//             h1: h1,
//             h2: h2,
//             i: 0,
//             count: self.num_hashes,
//         }
//     }

// }


#[derive(Clone)]
pub struct BloomFile {
    h1: AHasher,
    h2: AHasher,
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
            h1: AHasher::new_with_keys(key1,0),
            h2: AHasher::new_with_keys(key2,0),
            key1: key1,
            key2: key2,
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
            f.write_at(&byte,h/8).expect("Unable to write data");

            
            // let mut byte = [0u8];
            // let r = File::open(FILE_NAME).unwrap();
            // r.read_at(&mut byte, h/8).expect("Unable to read data");
            // let mut delta = 0b00000001u8;
            // delta <<= h%8;
            // if (byte[0] & delta) == 0b00000000u8 {
            //     panic!("failed to write correctly")
            // }
        }
    }
    pub fn contains(&self, item: &[u8;32]) -> bool {
        for h in self.get_hashes(item) {
            let h = h % FILE_SIZE;
            let mut byte = [0u8];
            let r = File::open(FILE_NAME).unwrap();
            r.read_at(&mut byte, h/8).expect("Unable to read data");
            let mut delta = 0b00000001u8;
            delta <<= h%8;
            if (byte[0] & delta) == 0b00000000u8 {
                return false;
            }
        }
        true
    }

    fn get_hashes(&self, item: &[u8;32]) -> HashIter {
        let mut h1 = self.h1.clone();
        // let mut h2 = self.h2.clone();
        h1.write(item);
        // h2.write(item);
        // let h1 = h1.finish();
        // let h2 = h2.finish();
        HashIter {
            h: h1,
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
}
