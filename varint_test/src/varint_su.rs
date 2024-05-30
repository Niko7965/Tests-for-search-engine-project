use std::mem;

pub struct VarintSU {
    pub bytes: Box<[u8]>,
    len: u32,
}

impl VarintSU {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter {
            int_vec: &self.bytes,
            next_index: 0,
            last_value: 0,
        }
    }
}

pub struct Iter<'a> {
    int_vec: &'a [u8],
    next_index: usize,
    last_value: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.int_vec.len() <= self.next_index {
            return None;
        }

        let mut x: usize = 0;
        let mut p = 1;
        let mut b_word = self.int_vec[self.next_index] as usize;

        for _ in 0..4 {
            if b_word < 128 {
                break;
            }
            //println!("{b_word}");
            x += (b_word - 127) * p;
            p *= 128;
            self.next_index += 1;
            b_word = self.int_vec[self.next_index] as usize;
        }
        //println!("{b_word}");
        x = x + (b_word + 1) * p + self.last_value;
        self.last_value = x;
        self.next_index += 1;

        Some(x)
    }
}

pub struct VarintSUFactory {
    pub vec: Vec<u8>,
    top: u32,
    len: u32,
}
impl VarintSUFactory {
    pub fn new() -> Self {
        VarintSUFactory {
            vec: Vec::new(),
            top: 0,
            len: 0,
        }
    }

    pub fn into_varint_su(&mut self) -> VarintSU {
        let vec = mem::replace(&mut self.vec, Vec::new());

        VarintSU {
            bytes: vec.into_boxed_slice(),
            len: self.len,
        }
    }

    pub fn push_if_not_on_top(&mut self, int: u32) {
        if int != self.top {
            self.push_int(int);
        }
    }

    //if x >= 128, it can be written as x = c*128+d, where d < 128. We write d in a byte, and write c, recursively
    pub fn push_int(&mut self, int: u32) {
        if int == self.top {
            return;
        }

        self.len += 1;
        let mut x = int - self.top - 1;

        for _ in 0..4 {
            if x < 128 {
                break;
            }
            //println!("{x}");
            self.vec.push((128 + (x % 128)) as u8);
            //self.vec.push((128 + (x & 127)) as u8);
            //x = (x >> 7) - 1;
            x = (x / 128) - 1;
        }

        //println!("{x}");
        self.top = int;
        self.vec.push(x as u8);
    }
}

#[test]
fn test_compressions() {
    let mut fact = VarintSUFactory::new();
    fact.push_int(200);
    fact.push_int(17003);

    let varint = fact.into_varint_su();

    let mut iterator = varint.iter();
    assert_eq!(iterator.next().unwrap(), 200);
    assert_eq!(iterator.next().unwrap(), 17003);
}

#[test]
fn test_bench() {
    println!("{}", 357 & 127);
    println!("{}", 4 << 2);
}
