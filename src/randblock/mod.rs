use crate::account::{Account, OTAccount};
use curve25519_dalek::scalar::Scalar;
use crate::transaction::*;
use rand::Rng;
use std::convert::{TryInto};
use rayon::prelude::*;
use crate::ringmaker::{generate_ring, recieve_ring};
use crate::seal::BETA;



pub fn random_tx_set(n: &usize) -> Vec<Transaction> {


    (0..*n).into_par_iter().map(|x| {
        let whofrom = Account::new(&format!("{}",x));
        let whoto = Account::new(&format!("{}",(x+1)%n));

        let mut rng = rand::thread_rng();
        let amnt: u64 = rng.gen::<u64>()%(2u64.pow(BETA as u32)); // beta for monero is 64
        // let become_stkr: u8 = rng.gen();
        let mut stk = 0u64;
        let condition = x<*n;//(become_stkr > 64) & (amnt > 0);
        if condition {stk = rng.gen::<u64>()%amnt;}
        let out: u64 = rng.gen(); let out = if amnt-stk > 0 {out%(amnt-stk)} else {0u64};
        let fee: u64 = amnt-stk-out;
        let mut otas_creators = Vec::<OTAccount>::new();
        otas_creators.push(whofrom.derive_ot(&Scalar::from(amnt)));

        let mut outs = Vec::<(Account, Scalar)>::new();
        if condition {outs.push((whofrom.stake_acc(), Scalar::from(stk)));}
        outs.push((whoto, Scalar::from(out)));


        Transaction::spend(&otas_creators, &outs.iter().map(|(a,v)|(a,v)).collect(), &get_test_ring(5), &Scalar::from(fee),)
    }).collect::<Vec<Transaction>>()
}

pub fn random_polytx_set(n: &usize, y: &Vec<OTAccount>, oldheight: &u64) -> Vec<PolynomialTransaction> {

    let height = y.len() as u64;
    (0..*n).into_iter().map(|x| {
        let whofrom = Account::new(&format!("{}",x%n));
        let whoto = Account::new(&format!("{}",(x+1)%n));


        let mut otas_creators = Vec::<OTAccount>::new();
        otas_creators.push(whofrom.receive_ot(&y[((x+n-1)%n)+*oldheight as usize]).unwrap());

        let mut rng = rand::thread_rng();
        let amnt = u64::from_le_bytes(otas_creators[0].com.amount.unwrap().as_bytes()[..8].try_into().unwrap());
        let mut stk = 0u64;
        let condition = x<*n;//(become_stkr > 64) & (amnt > 0);
        if condition {stk = rng.gen::<u64>()%amnt;}
        let out: u64 = rng.gen(); let out = if amnt-stk > 0 {out%(amnt-stk)} else {0u64};

        let mut outs = Vec::<(Account, Scalar)>::new();
        if condition {outs.push((whofrom.stake_acc(), Scalar::from(stk)));}
        outs.push((whoto, Scalar::from(out)));

        let ringsize = 5;
        let rname = generate_ring(&vec![((x+n-1)%n)+*oldheight as usize], &ringsize, &height);
        let ring = recieve_ring(&rname).unwrap();
        /* vvv this is where people send you the ring members  vvv */ 
        let mut rlring = ring.par_iter().map(|x| y[*x as usize].to_owned()).collect::<Vec<OTAccount>>();
        /* ^^^ this is where people send you the ring members  ^^^ */ 
        rlring = rlring.into_par_iter().zip(&ring).map(|(y,i)|
            if *i == ((x+n-1)%n) as u64+*oldheight {whofrom.receive_ot(&y).unwrap()}
            else {y.publish_offer()}
        ).collect::<Vec<OTAccount>>();

        // let x = Transaction::spend_ring(&rlring, &outs.iter().map(|(a,v)|(a,v)).collect(),)
        // .polyform(&rname);
        // x.verify(&y).unwrap();
        // x

        Transaction::spend_ring(&rlring, &outs.iter().map(|(a,v)|(a,v)).collect(),)
        .polyform(&rname)
    }).collect::<Vec<PolynomialTransaction>>()
}
