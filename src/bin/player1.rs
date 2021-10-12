use std::{fs::File, io::Read};

use curve25519_dalek::scalar::Scalar;
use kora::{account::Account, validation::NextBlock};
use sha3::{Digest, Sha3_512};






/// this is different from the one in the gui (b then a then c)
fn get_pswrd(a: &String, b: &String, c: &String) -> Vec<u8> {
    println!("{}",b);
    println!("{}",a);
    println!("{}",c);
    let mut hasher = Sha3_512::new();
    hasher.update(&b.as_bytes());
    hasher.update(&a.as_bytes());
    hasher.update(&c.as_bytes());
    Scalar::from_hash(hasher).as_bytes().to_vec()
}

/// we'll hard code this into initial history so they users cant see the passwords
/// this is also the place to test code
fn main() {

    let person0 = get_pswrd(&"1234".to_string(),&"1234567".to_string(),&"12345".to_string());
    let leader = Account::new(&person0).stake_acc().derive_stk_ot(&Scalar::one()).pk.compress();
    println!("{:?}",leader);

    for b in 0u64..10u64 {
        let file = format!("blocks/b{}",b);
        println!("checking for file {:?}...",file);
        if let Ok(mut file) = File::open(file) {
            let mut x = vec![];
            file.read_to_end(&mut x).unwrap();
            assert!(x == NextBlock::read(&b).unwrap());
        } else {
            assert!(NextBlock::read(&b).is_err());
        }
    }




}