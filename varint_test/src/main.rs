use std::{hint::black_box, time::Instant};

use itertools::Itertools;
use rand::Rng;
use varint_gb::{DescriptorTable, VarintGBFactory};
use varint_su::VarintSUFactory;

mod varint_gb;
mod varint_su;

fn main() {
    const SIZE: usize = 20000000;

    let no_of_inserts = SIZE;
    let mut reference_vector = Vec::new();
    let mut seq_su_fact = VarintSUFactory::new();
    let mut seq_gb_fact = VarintGBFactory::new();
    let shuffle_table = DescriptorTable::new();

    let mut rng = rand::thread_rng();
    for _ in 0..no_of_inserts {
        reference_vector.push(rng.gen_range(1..no_of_inserts) as u32);
    }

    reference_vector.sort();
    reference_vector = reference_vector.iter().unique().map(|i| *i).collect();

    //SU PUSH
    let su_start_push = Instant::now();
    for val in reference_vector.iter() {
        seq_su_fact.push_int(*val);
    }
    let seq_su = seq_su_fact.into_varint_su();
    let su_push_time = su_start_push.elapsed();

    //GB PUSH
    let gb_start_push = Instant::now();
    for val in reference_vector.iter() {
        seq_gb_fact.push_int(*val);
    }
    let seq_gb = seq_gb_fact.into_varint_gb();
    let gb_push_time = gb_start_push.elapsed();

    let su_start_decode = Instant::now();
    for val in seq_su.iter() {
        black_box(val);
    }
    let su_decode_time = su_start_decode.elapsed();

    let gb_start_decode = Instant::now();

    for chunk in seq_gb.iter(&shuffle_table) {
        for val in chunk {
            black_box(val);
        }
    }

    let gb_decode_time = gb_start_decode.elapsed();

    let ref_start_decode = Instant::now();
    for val in reference_vector.iter() {
        black_box(val);
    }
    let ref_decode_time = ref_start_decode.elapsed();

    println!("SU: ");
    println!(
        "Push-time: {}, decode-time: {}",
        su_push_time.as_millis(),
        su_decode_time.as_millis()
    );

    println!("GB: ");
    println!(
        "Push-time: {}, decode-time: {}",
        gb_push_time.as_millis(),
        gb_decode_time.as_millis()
    );

    println!("REF: ");
    println!("Decode-time: {}", ref_decode_time.as_millis());

    println!(" ");
}
