use std::arch::x86_64::_mm_loadu_si128;
use std::{
    arch::x86_64::{__m128i, _mm_shuffle_epi8},
    ptr,
};

/*
This refers to an implementation of a compressed integer sequence, with integer lengths described in Grouped Binary

We store integers in groups of four, grouped together with one descriptor byte:

[Value 4] [Value 3] [Value 2] [Value 1] [Descriptor]

The descriptor byte stores the size of each value in 1-4 byte:
[00-01-10-11]
[Bytes in value 4 - Bytes in value 3 - Bytes in value 2 - Bytes in value 1]

We use SIMD functions to decode.
*/

pub struct VarintGB {
    pub byte_stream: Box<[u8]>,
    len: u32,
}

impl VarintGB {
    pub fn new() -> Self {
        VarintGB {
            byte_stream: Vec::new().into_boxed_slice(),
            len: 0,
        }
    }

    pub fn iter<'a, 'b>(&'a self, shuffle_table: &'b DescriptorTable) -> Iter<'a, 'b> {
        Iter {
            descriptor_table: shuffle_table,
            byte_stream: &self.byte_stream,
            descriptor_index: 0,
            last_top: 0,
            len: self.len,
        }
    }

    #[allow(dead_code)]
    pub fn get_values(&self, descriptor_table: &DescriptorTable) -> Vec<u32> {
        let mut output = Vec::with_capacity(self.len());
        let mut descriptor_index = 0;
        while descriptor_index < self.byte_stream.len() {
            let descriptor = self.byte_stream[descriptor_index];
            let desc_entry = descriptor_table.get_entry_for_descriptor(descriptor);

            if descriptor_index + 17 >= self.byte_stream.len() {
                let chunk_byte_stream = &self.byte_stream[descriptor_index + 1..];
                descriptor_index += (desc_entry.length + 1) as usize;
                let delta_chunk = decode_chunk_safe_non_simd(descriptor, chunk_byte_stream);

                for val in delta_chunk {
                    if val == 0 {
                        return output;
                    }
                    output.push(val + output.last().unwrap_or(&0));
                }

                continue;
            }

            let chunk_addr = ptr::addr_of!(self.byte_stream[descriptor_index + 1]) as *mut __m128i;
            let delta_chunk = decode_chunk_by_address(chunk_addr, desc_entry.shuffle_sequence);
            for val in delta_chunk {
                output.push(val + output.last().unwrap_or(&0));
            }
            descriptor_index += (desc_entry.length + 1) as usize;
        }
        return output;
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }
}

pub struct VarintGBFactory {
    byte_stream: Vec<u8>,
    top: u32,
    descriptor_index: usize,
    index_in_chunk: u8,
    bytes_in_current_chunk: u8,
    no_of_chunks: u32,
    len: u32,
}
impl VarintGBFactory {
    pub fn new() -> Self {
        VarintGBFactory {
            byte_stream: Vec::new(),
            top: 0,
            descriptor_index: 0,
            index_in_chunk: 0,
            bytes_in_current_chunk: 0,
            no_of_chunks: 0,
            len: 0,
        }
    }

    pub fn get_top(&self) -> u32 {
        self.top
    }

    pub fn push_if_not_on_top(&mut self, num: u32) {
        if self.top != num {
            self.push_int(num);
        }
    }
    pub fn push_int(&mut self, x: u32) {
        self.len += 1;

        //If starting a new chunk, add descriptor, reset counter, and increment no of chunks
        if self.index_in_chunk == 0 {
            self.byte_stream.push(0);
            self.descriptor_index = self.byte_stream.len() - 1;
            self.bytes_in_current_chunk = 0;
            self.no_of_chunks += 1;
        }

        let delta = x - self.top;
        self.top = x;

        //Transmute to a slice of bytes
        let x_bytes_sized: [u8; 4] = delta.to_ne_bytes();

        //Gives os a reference to the bytes instead, allowing us to change the size
        let mut x_bytes: &[u8] = &x_bytes_sized;

        //Remove trailing 0 bytes.
        //fx 2: 0000-0000 0000-0000 0000-0000 0000-0010
        //becomes 0000-0010
        while x_bytes.len() > 1 && x_bytes[x_bytes.len() - 1] == 0 {
            x_bytes = &x_bytes[..x_bytes.len() - 1];
        }

        //Push the bytes to the bitstream
        for byte in x_bytes {
            self.byte_stream.push(*byte);
        }

        //We push the length of the int to the descriptor
        let mut int_len = (x_bytes.len() - 1) as u8;
        self.bytes_in_current_chunk += int_len;

        //We rotate it, such that it fits in the right place in the descriptor:
        int_len <<= self.index_in_chunk * 2;

        self.byte_stream[self.descriptor_index] ^= int_len;

        self.index_in_chunk = (self.index_in_chunk + 1) % 4;
    }

    pub fn into_varint_gb(&mut self) -> VarintGB {
        VarintGB {
            byte_stream: std::mem::replace(&mut self.byte_stream, Vec::new()).into_boxed_slice(),
            len: self.len,
        }
    }
}

pub fn decode_chunk(chunk: &[u8; 16], shuffle_sequence: __m128i) -> [u32; 4] {
    let unshufled_array: [u32; 4];
    //let four_numbers: [u8; 16] = chunk[..16].try_into().ok().unwrap();

    unsafe {
        let four_numbers_simd_vec: __m128i = std::mem::transmute(*chunk);
        let unshufled_vector = _mm_shuffle_epi8(four_numbers_simd_vec, shuffle_sequence);
        unshufled_array = std::mem::transmute(unshufled_vector);
        //let output_address_pointer = std::mem::transmute(output_address);
        //_mm_storeu_si128(output_address_pointer, unshufled_vector);
    }

    unshufled_array
}

#[inline(always)]
pub fn decode_chunk_by_address(chunk_addr: *mut __m128i, shuffle_sequence: __m128i) -> [u32; 4] {
    unsafe {
        let four_numbers_simd_vec: __m128i = _mm_loadu_si128(chunk_addr);
        let unshufled_vector = _mm_shuffle_epi8(four_numbers_simd_vec, shuffle_sequence);
        return std::mem::transmute(unshufled_vector);
    }
}

#[target_feature(enable = "sse3")]
pub unsafe fn decode_chunk_to(
    chunk: &[u8; 16],
    shuffle_sequence: __m128i,
    destination_vec: &mut Vec<__m128i>,
) {
    unsafe {
        let four_numbers_simd_vec: __m128i = std::mem::transmute(*chunk);
        destination_vec.push(_mm_shuffle_epi8(four_numbers_simd_vec, shuffle_sequence));
    }
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx2"
))]
pub unsafe fn decode_chunk_to_address(
    chunk: &[u8; 16],
    shuffle_sequence: __m128i,
    destination_address: *mut __m128i,
) {
    unsafe {
        let four_numbers_simd_vec: __m128i = std::mem::transmute(*chunk);
        let unshuffled = _mm_shuffle_epi8(four_numbers_simd_vec, shuffle_sequence);
        _mm_storeu_si128(destination_address, unshuffled);
    }
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx2"
))]
pub unsafe fn decode_chunk_by_address_to_address(
    chunk_addr: *mut __m128i,
    shuffle_sequence: __m128i,
    destination_address: *mut __m128i,
) {
    use std::arch::x86_64::_mm_loadu_si128;

    unsafe {
        let four_numbers_simd_vec: __m128i = _mm_loadu_si128(chunk_addr);
        let unshuffled = _mm_shuffle_epi8(four_numbers_simd_vec, shuffle_sequence);
        _mm_storeu_si128(destination_address, unshuffled);
    }
}

fn shuffle_sequence_from_descriptor(descriptor: u8) -> [i8; 16] {
    let mut word_index = 0;
    let mut shuffle_index = 0;
    let mut shuffle = [0_i8; 16];

    //For each word in the descriptor
    for i in 0..4 {
        //For each output byte corresponding to the word
        let word_len = descriptor_length_i(descriptor, i);
        for n in 0..4 {
            if n < word_len {
                shuffle[shuffle_index] = word_index;
                word_index += 1;
            } else {
                shuffle[shuffle_index] = -1;
            }
            shuffle_index += 1;
        }
    }

    shuffle
}

fn deltas_to_values(deltas: &[u32]) -> Vec<u32> {
    let mut values = Vec::with_capacity(deltas.len());
    let mut last = 0;
    for delta in deltas.iter() {
        if *delta == 0 {
            continue;
        }
        last += delta;
        values.push(last);
    }
    values
}

fn descriptor_length_i(descriptor: u8, index: usize) -> u8 {
    let mut mask = 0b00000011u8;
    mask <<= index * 2;
    let len = descriptor & mask;
    (len >> (index * 2)) + 1
}

fn descriptor_length_total(descriptor: u8) -> u8 {
    let mut length = 0;
    for i in 0..4 {
        length += descriptor_length_i(descriptor, i);
    }
    length
}

#[derive(Copy, Clone)]
pub struct DescriptorEntry {
    shuffle_sequence: __m128i,
    length: u8,
}

pub struct DescriptorTable {
    table: Vec<DescriptorEntry>,
}

impl DescriptorTable {
    pub fn new() -> Self {
        let mut table = Vec::with_capacity(256);
        for descriptor in 0..=255 {
            table.push(Self::create_entry_for_descriptor(descriptor))
        }

        DescriptorTable { table }
    }

    fn create_entry_for_descriptor(descriptor: u8) -> DescriptorEntry {
        let shf = unsafe { std::mem::transmute(shuffle_sequence_from_descriptor(descriptor)) };
        let length = descriptor_length_total(descriptor);
        DescriptorEntry {
            shuffle_sequence: shf,
            length,
        }
    }

    #[inline(always)]
    pub fn get_entry_for_descriptor(&self, descriptor: u8) -> DescriptorEntry {
        self.table[descriptor as usize]
    }

    pub fn get_shuffle_for_descriptor(&mut self, descriptor: u8) -> __m128i {
        self.get_entry_for_descriptor(descriptor).shuffle_sequence
    }

    #[allow(dead_code)]
    pub fn get_length_for_descriptor(&mut self, descriptor: u8) -> u8 {
        self.get_entry_for_descriptor(descriptor).length
    }
}

#[inline(always)]
pub fn decode_chunk_safe_non_simd(descriptor: u8, byte_stream: &[u8]) -> [u32; 4] {
    let mut chunk = [0; 4];
    let mut index = 0;
    for i in 0..4 {
        let len = descriptor_length_i(descriptor, i);
        let mut num = 0;
        for pos in 0..len {
            if index >= byte_stream.len() {
                return chunk;
            }
            num += (byte_stream[index] as u32) << pos * 8;
            index += 1;
        }
        chunk[i] = num;
    }
    chunk
}

#[test]
pub fn test_safe_decoder_non_simd() {
    let byte_stream = [0b11111001, 0b11111111, 1, 1, 1];
    let descriptor = 0b00000001u8;
    let vals = decode_chunk_safe_non_simd(descriptor, &byte_stream);
    for val in vals {
        println!("{val}");
    }

    let u16 = 0b1111100111111111;
    let u16_2 = 0b1111111111111100;
    println!("u16: {u16_2}");
}

pub struct Iter<'a, 'b> {
    descriptor_table: &'b DescriptorTable,
    byte_stream: &'a [u8],
    len: u32,
    descriptor_index: usize,
    last_top: u32,
}
impl Iter<'_, '_> {
    pub fn len(&self) -> usize {
        self.len as usize
    }
}

impl<'a, 'b> Iterator for Iter<'a, 'b> {
    type Item = [u32; 4];

    fn next(&mut self) -> Option<Self::Item> {
        if self.descriptor_index >= self.byte_stream.len() {
            return None;
        }

        let descriptor = self.byte_stream[self.descriptor_index];

        let desc_entry = self.descriptor_table.get_entry_for_descriptor(descriptor);

        //If there arent 16 more bytes to take
        if self.descriptor_index + 17 >= self.byte_stream.len() {
            let chunk_byte_stream = &self.byte_stream[self.descriptor_index + 1..];
            self.descriptor_index += (desc_entry.length + 1) as usize;
            let mut delta_chunk = decode_chunk_safe_non_simd(descriptor, chunk_byte_stream);
            delta_chunk_to_value_chunk(&mut delta_chunk, self.last_top);
            self.last_top = delta_chunk[3];
            return Some(delta_chunk);
        }

        let chunk_addr = ptr::addr_of!(self.byte_stream[self.descriptor_index + 1]) as *mut __m128i;
        let mut delta_chunk = decode_chunk_by_address(chunk_addr, desc_entry.shuffle_sequence);

        /*
        let chunk = <&[u8; 16]>::try_from(
            &self.byte_stream[self.descriptor_index + 1..self.descriptor_index + 17],
        )
        .unwrap();
        let mut delta_chunk = decode_chunk(chunk, desc_entry.shuffle_sequence);
        */

        self.descriptor_index += (desc_entry.length + 1) as usize;

        delta_chunk_to_value_chunk(&mut delta_chunk, self.last_top);
        self.last_top = delta_chunk[3];

        Some(delta_chunk)
    }
}

#[inline(always)]
fn delta_chunk_to_value_chunk(delta_chunk: &mut [u32; 4], last_top: u32) {
    delta_chunk[0] += last_top;
    delta_chunk[1] += delta_chunk[0];
    delta_chunk[2] += delta_chunk[1];
    delta_chunk[3] += delta_chunk[2];
}

fn print_vec(v: &Vec<u32>) {
    for (i, val) in v.iter().enumerate() {
        println!("{i} : {val}");
    }
}

#[cfg(test)]
mod tests {
    use std::{arch::x86_64::__m128i, hint::black_box, ptr, time::Instant};

    use itertools::Itertools;
    use rand::Rng;

    use crate::{varint_gb::descriptor_length_i, varint_su::VarintSUFactory};

    use super::{
        decode_chunk, decode_chunk_by_address, deltas_to_values, shuffle_sequence_from_descriptor,
        DescriptorTable, VarintGB, VarintGBFactory,
    };

    #[test]
    fn test_of_sequence() {
        let no_of_inserts = 100000;
        let mut reference_vector = Vec::new();
        let mut seq_factory = VarintGBFactory::new();

        let mut rng = rand::thread_rng();
        for _ in 0..no_of_inserts {
            reference_vector.push(rng.gen_range(1..no_of_inserts) as u32);
        }

        reference_vector.sort();
        reference_vector = reference_vector.iter().unique().map(|i| *i).collect();

        for val in reference_vector.iter() {
            seq_factory.push_int(*val);
        }
        let seq = seq_factory.into_varint_gb();

        let mut shuffle_table = DescriptorTable::new();

        let seq_values_iter = seq.iter(&mut shuffle_table);
        let mut reference_iter = reference_vector.iter();

        for chunk in seq_values_iter {
            for num in chunk {
                if let Some(ref_num) = reference_iter.next() {
                    assert_eq!(num, *ref_num);
                }
            }
        }
        //println!("{},{}", seq_values.len(), reference_vector.len());
    }

    #[test]
    fn test_descriptor_length_i() {
        let descriptor = 0b00011011u8;
        for i in 0..4 {
            assert_eq!(4 - i, descriptor_length_i(descriptor, i) as usize);
        }
    }

    #[test]
    fn test_delta_conversion() {
        let mut v = Vec::new();
        v.push(1);
        v.push(1);
        v.push(1);

        let v = deltas_to_values(&v);
        for i in 0..3 {
            assert_eq!(v[i] as usize, i + 1);
        }
    }

    #[test]
    fn test_len() {
        let mut v = Vec::new();
        v.push(1);
        v.push(1);
        v.push(1);
        v.push(2);
        v.push(5);

        assert_eq!(v.len(), 5);
    }

    #[test]
    fn test_chunk_decoder() {
        let mut chunk_vec = Vec::new();
        let descriptor = 0b00000100u8;

        chunk_vec.push(1);

        chunk_vec.push(0);
        chunk_vec.push(1);

        chunk_vec.push(4);
        chunk_vec.push(5);

        while chunk_vec.len() < 16 {
            chunk_vec.push(0);
        }

        let chunk_16 = &chunk_vec.try_into().ok().unwrap();
        let output = decode_chunk(
            chunk_16,
            DescriptorTable::new().get_shuffle_for_descriptor(descriptor),
        );
        for i in output {
            println!("{}", i);
        }

        let output2 = decode_chunk_by_address(
            ptr::addr_of!(chunk_16[0]) as *mut __m128i,
            DescriptorTable::new().get_shuffle_for_descriptor(descriptor),
        );
        for i in output2 {
            println!("{}", i);
        }
    }

    #[test]
    fn test_bench() {
        let mut cis_gb_fact = VarintGBFactory::new();
        cis_gb_fact.push_int(1);
        cis_gb_fact.push_int(2);
        cis_gb_fact.push_int(3);
        cis_gb_fact.push_int(4);

        cis_gb_fact.push_int(5);
        cis_gb_fact.push_int(6);
        cis_gb_fact.push_int(7);
        cis_gb_fact.push_int(8);

        cis_gb_fact.push_int(65536 + 1);
        cis_gb_fact.push_int(65536 + 2);
        cis_gb_fact.push_int(65536 + 3);
        cis_gb_fact.push_int(65536 + 4);
        cis_gb_fact.push_int(65536 + 5);

        println!("{}", cis_gb_fact.no_of_chunks);

        let varint_gb = cis_gb_fact.into_varint_gb();
        println!("Bytes:");
        for (index, byte) in varint_gb.byte_stream.iter().enumerate() {
            println!("{index} : {byte:#08b}");
        }

        let mut shuffle_table = DescriptorTable::new();

        println!("Nums unsafe:");
        for chunk in varint_gb.iter(&mut shuffle_table) {
            for num in chunk {
                println!("{}", num);
            }
        }
    }
}
