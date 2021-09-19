use std::{fs, io::Read};

use kora::validation::NextBlock;


fn main() {
    let mut f  = fs::File::open("blocks/b1").unwrap();
    let mut b = vec![]; 
    f.read_to_end(&mut b).unwrap();
    let b = bincode::deserialize::<NextBlock>(&b).unwrap();
    println!("{:?}",b);

}