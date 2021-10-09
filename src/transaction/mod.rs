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
use crate::commitment::Commitment;


use curve25519_dalek::ristretto::CompressedRistretto;


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
            outputs: outputs.iter().map(|x| x.publish_offer()).collect::<Vec<_>>(),
            tags: tagelem,
            seal,
            fee: u64::from_le_bytes(fee_amount.as_bytes()[..8].try_into().unwrap()),
        }
    }
    
    pub fn spend_ring(inring: &Vec<OTAccount>, recipients: &Vec<(&Account, &Scalar)>) -> Transaction{
        let (poss,inamnt): (Vec<usize>,Vec<Scalar>) = inring.iter().enumerate().filter_map(|(i,a)|if let Some(x) = a.com.amount {Some((i,x))} else {None}).unzip();

        // println!("in amount: {:?}",inamnt);

        let ring = inring.to_owned();
        let fee_amount = inamnt.into_iter().sum::<Scalar>() - recipients.iter().map(|(_,y)| y.to_owned()).sum::<Scalar>();
        let mut outputs = recipients.into_iter().map(|(rcpt,amout)|
            if rcpt.vpk == RISTRETTO_BASEPOINT_POINT {rcpt.derive_stk_ot(amout)}
            else {rcpt.derive_ot(amout)}
        ).collect::<Vec<OTAccount>>();
        outputs.push(fee_ota(&fee_amount));

        // println!("fee: {:?}",fee_amount);

        let inputs:Vec<OTAccount> = ring.iter().map(|acct|(acct.clone())).collect();
        let sigin:Vec<&OTAccount> = ring.iter().map(|acct|acct).collect();
        let sigout:Vec<&OTAccount> = outputs.iter().map(|acct|acct).collect();
        let mut tr = Transcript::new(b"seal tx");

        let tagelem: Vec<Tag> = poss.iter().map(|pos| ring[*pos].clone()).map(|acct| acct.get_tag().unwrap().clone()).collect();
        let tags: Vec<&Tag> = tagelem.iter().map(|t|t).collect();
        let seal = SealSig::sign(&mut tr, &sigin, &tags, &poss, &sigout).expect("Not able sign tx");
        outputs.pop();

        outputs.iter_mut().for_each(|x| {*x = x.publish_offer();});
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
        let inputs: Vec<&OTAccount> = self.inputs.iter().map(|a| a).collect();
        let tags: Vec<&Tag> = self.tags.iter().map(|a| a).collect();
        
        let mut outputs = self.outputs.clone();
        outputs.push(fee_ota(&Scalar::from(self.fee)));
        let outputs: Vec<&OTAccount> = outputs.iter().map(|a|a).collect();
        
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






#[derive(Default, Clone, Eq, Serialize, Deserialize, Hash, Debug)]
pub struct PolynomialTransaction{
    pub inputs: Vec<u8>,
    pub outputs: Vec<OTAccount>,
    pub tags: Vec<Tag>,
    pub seal: SealSig,
    pub fee: u64,
}
impl PartialEq for PolynomialTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.inputs == other.inputs && self.outputs == other.outputs  && self.tags == other.tags && self.seal == other.seal && self.fee == other.fee
    }
}
impl PolynomialTransaction {
    pub fn verify_ram(&self,history:&Vec<OTAccount>) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.iter().map(|a| a).collect();
        if let Ok(i) = recieve_ring(&self.inputs) {
            let inputs: Vec<&OTAccount> = i.iter().map(|x| &history[*x as usize]).collect();        
            let mut outputs = self.outputs.clone();
            outputs.push(fee_ota(&Scalar::from(self.fee)));
            let outputs: Vec<&OTAccount> = outputs.iter().map(|a|a).collect();
            
            let b = self.seal.verify(&mut tr, &inputs, &tags, &outputs);

            match b {
                Ok(()) => Ok(()),
                Err(_) => Err(TransactionError::InvalidTransaction)
            }
        } else {
            Err(TransactionError::InvalidTransaction)
        }
    }

    pub fn verify(&self) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.iter().map(|a| a).collect();
        if let Ok(i) = recieve_ring(&self.inputs) {
            let inputs = i.iter().map(|x| OTAccount::summon_ota(&History::get(x))).collect::<Vec<OTAccount>>();        
            let mut outputs = self.outputs.clone();
            outputs.push(fee_ota(&Scalar::from(self.fee)));
            let outputs: Vec<&OTAccount> = outputs.iter().map(|a|a).collect();
            
            let b = self.seal.verify(&mut tr, &inputs.iter().collect::<Vec<_>>(), &tags, &outputs);

            match b {
                Ok(()) => Ok(()),
                Err(_) => Err(TransactionError::InvalidTransaction)
            }
        } else {
            Err(TransactionError::InvalidTransaction)
        }
    }

    pub fn verifystk(&self,history:&Vec<(CompressedRistretto,u64)>) -> Result<(), TransactionError> {
        let mut tr = Transcript::new(b"seal tx");
        let tags: Vec<&Tag> = self.tags.iter().map(|a| a).collect();
        let mut i = self.inputs.clone();
        if i.pop() == Some(1) {
            let places = i.chunks_exact(8).map(|x| u64::from_le_bytes(x.try_into().unwrap()) as usize).collect::<Vec<_>>();
            if !places[1..].iter().enumerate().all(|(i,&x)| x > places[i]) {
                return Err(TransactionError::InvalidTransaction)
            }
            let mut input = vec![];
            for i in places {
                let (pk,amnt) = match history.get(i) {
                    Some(x) => x,
                    None => return Err(TransactionError::InvalidTransaction),
                };
                let com = Commitment::commit(&Scalar::from(*amnt),&Scalar::zero());
                input.push(OTAccount{pk: pk.decompress().unwrap(),com,..Default::default()});
            }

            let mut outputs = self.outputs.clone();
            outputs.push(fee_ota(&Scalar::from(self.fee)));
            let outputs: Vec<&OTAccount> = outputs.iter().map(|a|a).collect();
            
            let b = self.seal.verify(&mut tr, &input.iter().collect::<Vec<_>>(), &tags, &outputs);

            match b {
                Ok(()) => Ok(()),
                Err(_) => Err(TransactionError::InvalidTransaction)
            }
        } else {
            Err(TransactionError::InvalidTransaction)
        }
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
