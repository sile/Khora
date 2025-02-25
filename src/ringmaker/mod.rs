use rand::{Rng, distributions::Uniform};
use std::usize;
use polynomial_over_finite_prime_field::PolynomialOverP;
use modinverse::modinverse;
use std::convert::TryInto;
use sha3::{Digest, Sha3_512};




pub fn hash(t: &u32) -> i128 {
    let mut s = Sha3_512::new();
    s.update(&t.to_le_bytes());
    let c = s.finalize();
    i128::from_le_bytes(c.to_vec()[..16].try_into().unwrap())
}
const P: i128 = 9223372036854775783;//a little below 2^63 so hash approximatly finds a rand num in this field
pub fn generate_ring(s: &Vec<usize>, r: &u16, now: &u64) -> Vec<u8> {
    /*
    IF YOU WANT TO MAKE SURE EVERY MEMBER OF THE RING IS UNIQUE,
    YOU'D HAVE TO RUN THIS N TIMES
    */

    let now = *now;
    let s = s.to_owned();
    let r = *r as i128;



    /* h is the random number */
    let h: Vec<_> = (0..r).collect();
    let h: Vec<_> = h.iter().map(|x| {
        hash(&(*x as u32)) as u64 
        % P as u64} ).collect();
    let ringmems_init = Uniform::new(0,r);
    /* indeces I want to be at */
    let mut rng = rand::thread_rng();
    let mut j = Vec::<usize>::new();
    while j.len() < s.len() {
        let a = rng.sample(&ringmems_init) as usize;
        if !j.iter().any(|&i| i == a) {
            j.push(a);
        }
    }

    /* offset I want to be (0 mod now) */
    let shift: Vec<_> = (0..s.len()).map(|_| {let a: u64 = rng.gen();
        (a%(P as u64 / now as u64))*(now as u64)
    }).collect();
    
    /* y = h - l */
    let y: Vec<_> = (0..s.len()).map(|ji|
        PolynomialOverP::<i128>::new(vec![h[j[ji]] as i128], P)
        - PolynomialOverP::<i128>::new(vec![(shift[ji] as i128 + s[ji] as i128)],P)
    ).collect();
    

    /* POLYNOMIAL MAKER */
    let justx = PolynomialOverP::<i128>::new(vec![0, 1], P);
    let mut poly = PolynomialOverP::<i128>::new(vec![], P);
    for (x, y) in j.iter().zip(y) {
        let mut term = y; //1+2x+3x^2
        for &xi in j.iter() {
            if *x != xi {
                let mut top = PolynomialOverP::<i128>::new(vec![xi as i128], P);
                top = justx.clone() - top;
                let bot = PolynomialOverP::<i128>::new(vec![*x as i128], P)
                 + PolynomialOverP::<i128>::new(vec![P - xi as i128], P);
                let bot = modinverse(bot.coefs()[0], P).unwrap();
                let bot = PolynomialOverP::<i128>::new(vec![bot as i128], P);
                term = term*top*bot;
            }
        }
        poly = poly + term;
    }


    /* these next few lines are irrelevant (they just print the polynomial. feel free to uncomment it out) */
    // let mut places = Vec::<i128>::new();
    // for x in 0..r {
    //     let mut throwaway = poly.eval(&(x as i128));
    //     if throwaway < 0 {
    //         throwaway = throwaway + p;
    //     }
    //     let y = PolynomialOverP::<i128>::new(vec![throwaway], p);
    //     let lpos = PolynomialOverP::<i128>::new(vec![h[x as usize] as i128], p) - y;
    //     if  lpos.clone().coefs().len() > 0 {
    //         let mut lpos = lpos.coefs()[0];
    //         if lpos < 0 {lpos = lpos + p;}
    //         places.push(lpos%now as i128);
    //     }
    //     else {
    //         places.push(0);
    //     }
    // }
    // let a: Vec<Option<usize>> = s.iter().map(
    //     |x| places.iter().position(
    //         |y| y.clone() as usize == x.clone()
    //     )
    // ).collect();
    // println!("{:?}",a);
    // places.sort();
    // places.dedup();
    // println!("me values: {:?}",s);
    // println!("me places: {:?}",j);
    // println!("**************************");
    // println!("ring mems: {:?}",places);
    /* irrelevant portion over */






    let mut coefficients = Vec::<u64>::new();
    for mut c in poly.coefs() {
        if c < 0 {c = c+P;}
        coefficients.push(c as u64);
    }

    let mut send = Vec::<u8>::new();
    send.extend((r as u16).to_le_bytes());
    send.extend(now.to_le_bytes());
    for i in coefficients.clone() {
        send.extend(i.to_le_bytes());
    }
    send.push(0);
    /* send ring size, key, now, polynomial */

    send
}

pub fn recieve_ring(recieved: &Vec<u8>) -> Result<Vec<u64>,&'static str> {
    /* errors are spit out for non primes */
    // println!("{:?}",u64::MAX); //

    let mut recieved = recieved.to_owned();
    /* send the info over */
    if recieved.pop() == Some(0) {
        let r_bytes: Vec<u8> = recieved.drain(..2).collect(); //u16
        let r_bytes: Result<[u8;2],_> = r_bytes.try_into();
        let r = u16::from_le_bytes(r_bytes.unwrap());

        let now_bytes: Vec<u8> = recieved.drain(..8).collect(); //u32
        let now_bytes: Result<[u8;8],_> = now_bytes.try_into();
        let now = u64::from_le_bytes(now_bytes.unwrap()) as i128;



        let mut coefficients = Vec::<u64>::new();
        while recieved.len() > 0 {
            let ci: Vec<u8> = recieved.drain(..8).collect();
            let ci: [u8;8] = ci.try_into().unwrap();
            coefficients.push(u64::from_le_bytes(ci));
        }


        let coefficients = coefficients.iter().map(|x| *x as i128).collect();
        let poly = PolynomialOverP::<i128>::new(coefficients, P);
        /* these next 2 paragraphs of comments are good */
        let throwaway: Vec<_> = (0..r).collect();
        let h: Vec<_> = throwaway.iter().map(|x| {hash(&(*x as u32)) as u64 % P as u64} ).collect();
        let mut places = Vec::<i128>::new();
        for x in 0..r {
            let mut throwaway = poly.eval(&(x as i128));
            if throwaway < 0 {
                throwaway = throwaway + P;
            }
            let y = PolynomialOverP::<i128>::new(vec![throwaway], P);
            let lpos = PolynomialOverP::<i128>::new(vec![h[x as usize] as i128], P) - y;
            
            if  lpos.clone().coefs().len() > 0 {
                let mut lpos = lpos.coefs()[0];
                if lpos < 0 {lpos = lpos + P;}
                places.push(lpos%now as i128);
            }
            else {
                places.push(0);
            }

        }

        places.sort();
        places.dedup();
        Ok(places.into_iter().map(|x| x as u64).collect())
    } else {
        Err("this is not a regular transaction")
    }
}



