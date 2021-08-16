#![allow(dead_code)]
use std::convert::TryInto;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::scalar::Scalar;
use merlin::Transcript;
use serde::{Serialize, Deserialize};
use rand::random;

use crate::account::{OTAccount, Account, Tag, fee_ota};
use crate::seal::SealSig;
use crate::ringmaker::*;
use crate::commitment::{Commitment};


use curve25519_dalek::ristretto::{RistrettoPoint, CompressedRistretto};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, IntoParallelRefMutIterator};
use rayon::iter::ParallelIterator;
use rayon::iter::IntoParallelIterator;
use crate::validation::History;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum TransactionError{
    InvalidTransaction,
    InvalidOffer
}



#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Transaction{ // this used to have pub(crate) everywhere
    pub inputs: Vec<OTAccount>,
    pub outputs: Vec<OTAccount>,
    pub tags: Vec<Tag>,
    pub seal: SealSig,
    pub fee: u64,
}
impl Transaction {
    

    pub fn spend(accts: &Vec<OTAccount>, recipients: &Vec<(&Account, &Scalar)>, inring: &Vec<OTAccount>, fee_amount: &Scalar) -> Transaction{
        let mut poss = Vec::<usize>::new();
        let mut ring = inring.clone();
        for acct in accts.iter(){
            let mut pos = random::<usize>() % ring.len();
            while poss.contains(&pos) { // This  puts the 2 inps in random locations
                pos = random::<usize>() % ring.len();
            }
            ring[pos] = acct.clone();
            poss.push(pos);
        }
        let mut outputs = Vec::<OTAccount>::new();

        for (rcpt, amout) in recipients {
            if rcpt.vpk == RISTRETTO_BASEPOINT_POINT {
                outputs.push(rcpt.stake_acc().derive_stk_ot(amout));
            }
            else {
                outputs.push(rcpt.derive_ot(amout));
            }
        }
        outputs.push(fee_ota(fee_amount)); // let's move this into the seal function to make it prettier

        let inputs:Vec<OTAccount> = ring.iter().map(|acct|(acct.clone())).collect();
        let sigin:Vec<&OTAccount> = ring.iter().map(|acct|acct).collect();
        let sigout:Vec<&OTAccount> = outputs.iter().map(|acct|acct).collect();
        let mut tr = Transcript::new(b"seal tx");

        let tagelem: Vec<Tag> = poss.iter().map(|pos| ring[*pos].clone()).map(|acct| acct.get_tag().unwrap().clone()).collect();
        let tags: Vec<&Tag> = tagelem.iter().map(|t|t).collect();

        let seal = SealSig::sign(&mut tr, &sigin, &tags, &poss, &sigout).expect("Not able sign tx");
        outputs.pop(); // let's move this into the seal function to make it prettier
        Transaction{
            inputs ,
            outputs: outputs.par_iter().map(|x| x.publish_offer()).collect::<Vec<_>>(),
            tags: tagelem,
            seal,
            fee: u64::from_le_bytes(fee_amount.as_bytes()[..8].try_into().unwrap()),
        }
    }
    
    pub fn spend_ring(inring: &Vec<OTAccount>, recipients: &Vec<(&Account, &Scalar)>) -> Transaction{
        let (poss,inamnt): (Vec<usize>,Vec<Scalar>) = inring.par_iter().enumerate().filter_map(|(i,a)|if let Some(x) = a.com.amount {Some((i,x))} else {None}).unzip();

        let ring = inring.to_owned();
        let fee_amount = inamnt.into_par_iter().sum::<Scalar>() - recipients.par_iter().map(|(_,y)| y.to_owned()).sum::<Scalar>();
        let mut outputs = recipients.into_par_iter().map(|(rcpt,amout)|
            if rcpt.vpk == RISTRETTO_BASEPOINT_POINT {rcpt.derive_stk_ot(amout)}
            else {rcpt.derive_ot(amout)}
        ).collect::<Vec<OTAccount>>();
        outputs.push(fee_ota(&fee_amount));

        let inputs:Vec<OTAccount> = ring.iter().map(|acct|(acct.clone())).collect();
        let sigin:Vec<&OTAccount> = ring.iter().map(|acct|acct).collect();
        let sigout:Vec<&OTAccount> = outputs.iter().map(|acct|acct).collect();
        let mut tr = Transcript::new(b"seal tx");

        let tagelem: Vec<Tag> = poss.iter().map(|pos| ring[*pos].clone()).map(|acct| acct.get_tag().unwrap().clone()).collect();
        let tags: Vec<&Tag> = tagelem.iter().map(|t|t).collect();
        let seal = SealSig::sign(&mut tr, &sigin, &tags, &poss, &sigout).expect("Not able sign tx");
        outputs.pop();

        outputs.par_iter_mut().for_each(|x| {*x = x.publish_offer();});
        Transaction{
            inputs ,
            outputs,
            tags: tagelem,
            seal,
            fee: u64::from_le_bytes(fee_amount.as_bytes()[..8].try_into().unwrap()),
        }
    }


    pub fn verify(&self) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let inputs: Vec<&OTAccount> = self.inputs.par_iter().map(|a| a).collect();
        let tags: Vec<&Tag> = self.tags.par_iter().map(|a| a).collect();
        
        let mut outputs = self.outputs.clone();
        outputs.push(fee_ota(&Scalar::from(self.fee)));
        let outputs: Vec<&OTAccount> = outputs.par_iter().map(|a|a).collect();
        
        let b = self.seal.verify(&mut tr, &inputs, &tags, &outputs);

        match b {
            Ok(()) => Ok(()),
            Err(_) => Err(TransactionError::InvalidTransaction)
        }
    }

    pub fn try_receive(&self, acc: &Account) -> Vec<OTAccount> {
        let outputs: Vec<OTAccount> = self.outputs.clone();
        let mine: Vec<OTAccount> = outputs.into_iter().filter(|x| acc.receive_ot(x).is_ok()).collect();
        let accts: Vec<OTAccount> = mine.iter().map(|x| acc.receive_ot(x).unwrap()).collect();
        accts
    }

    pub fn polyform(&self,poly:&Vec<u8>) -> PolynomialTransaction {
        PolynomialTransaction{
            inputs:poly.to_owned(),
            outputs:self.outputs.to_owned(),
            tags:self.tags.to_owned(),
            seal:self.seal.to_owned(),
            fee:self.fee,
        }
    }
}

pub fn get_test_ring(n: usize) -> Vec<OTAccount> {
    let accounts = vec![OTAccount::default(); n];
    accounts
}






#[derive(Default, Clone, Serialize, Deserialize, Hash, Debug)]
pub struct PolynomialTransaction{
    pub inputs: Vec<u8>,
    pub outputs: Vec<OTAccount>,
    pub tags: Vec<Tag>,
    pub seal: SealSig,
    pub fee: u64,
}
impl PolynomialTransaction {
    pub fn verify_ram(&self,history:&Vec<OTAccount>) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.par_iter().map(|a| a).collect();
        let inputs: Vec<&OTAccount> = recieve_ring(&self.inputs).par_iter().map(|x| &history[*x as usize]).collect();        
        let mut outputs = self.outputs.clone();
        outputs.push(fee_ota(&Scalar::from(self.fee)));
        let outputs: Vec<&OTAccount> = outputs.par_iter().map(|a|a).collect();
        
        let b = self.seal.verify(&mut tr, &inputs, &tags, &outputs);

        match b {
            Ok(()) => Ok(()),
            Err(_) => Err(TransactionError::InvalidTransaction)
        }
    }

    pub fn verify(&self) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.par_iter().map(|a| a).collect();
        let inputs = recieve_ring(&self.inputs).par_iter().map(|x| OTAccount::summon_ota(&History::get(x))).collect::<Vec<OTAccount>>();        
        let mut outputs = self.outputs.clone();
        outputs.push(fee_ota(&Scalar::from(self.fee)));
        let outputs: Vec<&OTAccount> = outputs.par_iter().map(|a|a).collect();
        
        let b = self.seal.verify(&mut tr, &inputs.par_iter().collect::<Vec<_>>(), &tags, &outputs);

        match b {
            Ok(()) => Ok(()),
            Err(_) => Err(TransactionError::InvalidTransaction)
        }
    }

    pub fn verifystk(&self,history:&Vec<(CompressedRistretto,u64)>) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.par_iter().map(|a| a).collect();

        let (pk,amnt) = history[u64::from_le_bytes(self.inputs.to_owned().try_into().unwrap()) as usize];
        let com = Commitment::commit(&Scalar::from(amnt),&Scalar::zero());
        let input = OTAccount{pk: pk.decompress().unwrap(),com,..Default::default()};
        let mut outputs = self.outputs.clone();
        outputs.push(fee_ota(&Scalar::from(self.fee)));
        let outputs: Vec<&OTAccount> = outputs.par_iter().map(|a|a).collect();
        
        let b = self.seal.verify(&mut tr, &vec![&input], &tags, &outputs);

        match b {
            Ok(()) => Ok(()),
            Err(_) => Err(TransactionError::InvalidTransaction)
        }
    }

}
#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct SavedTransactionFull {
    pub outputs: Vec<OTAccount>,
    pub inputs: Vec<RistrettoPoint>,
    pub tags: Vec<Tag>,
    pub proof: SealSig,
    pub fee: u64,
}
impl SavedTransactionFull {
    pub fn from(tx: &Transaction) -> SavedTransactionFull {
        SavedTransactionFull {
            outputs: tx.outputs.to_owned(),
            inputs: tx.inputs.to_owned().into_par_iter().map(|x| x.pk).collect::<Vec<RistrettoPoint>>(),
            tags: tx.tags.to_owned(),
            proof: tx.seal.to_owned(),
            fee: tx.fee,
        }
    }
    pub fn shorten(&self) -> Vec<OTAccount> {
        self.outputs.to_owned()
    }
}

#[cfg(test)]
mod tests {
    #![allow(dead_code)]
    use super::*;

    #[test]
    fn create_tx() {

        let acct = Account::new(&"hi".to_string());
        let ota1 = acct.derive_ot(&Scalar::from(6u64));
        let ota2 = acct.derive_ot(&Scalar::from(10u64));
        let ota3 = acct.derive_ot(&Scalar::from(5u64));

        let tx = Transaction::spend(&vec![ota1,ota2,ota3], &vec![(&acct,&Scalar::from(6u64)),(&acct,&Scalar::from(3u64)),(&acct,&Scalar::from(12u64))], &get_test_ring(123),&Scalar::one());
        assert!(tx.verify().is_ok());
    }
}
