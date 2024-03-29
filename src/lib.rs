pub mod magic;

use core::clone::Clone;
use core::cmp::PartialOrd;
use log::debug;
use magic::{MAGIC_16, MAGIC_32, MAGIC_64, MAGIC_8};

use std::sync::Arc;

const BYTE_POS: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
const BYTE_RANGE: [u8; 3] = [2, 4, 8];

#[derive(Debug)]
pub struct TestCase {
    pub data: Vec<u8>,
    pub size: usize,
}

impl Default for TestCase {
    fn default() -> Self {
        TestCase {
            data: Vec::with_capacity(4096),
            size: 4096,
        }
    }
}

impl TestCase {
    pub fn new(data: &Vec<u8>) -> Self {
        TestCase {
            data: data.clone(),
            size: data.len(),
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn get_rdtsc() -> usize {
    unsafe { std::arch::x86_64::_rdtsc() as usize }
}

// https://lore.kernel.org/lkml/20200914115311.2201-3-leo.yan@linaro.org/
#[cfg(target_arch = "aarch64")]
fn get_rdtsc() -> usize {
    let ctr: u64 = 0;
    unsafe {
        asm!("mrs {x0}, cntvct_el0", inout(reg) ctr);
    }
    return ctr;
}

#[derive(Debug, Default)]
pub struct Rng(usize);

impl Rng {
    pub fn new(seed: usize) -> Self {
        if seed == 0 {
            Rng(0x5fd89eda3130256d ^ get_rdtsc())
        } else {
            Rng(seed)
        }
    }

    #[inline]
    pub fn rand(&mut self) -> usize {
        let value = self.0;
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 43;
        value
    }

    #[inline]
    pub fn gen_range(&mut self, min: usize, max: usize) -> usize {
        assert!(max >= min, "Failed bounds check");
        if min == max {
            return min;
        }
        if min == 0 && max == core::usize::MAX {
            return self.rand();
        }

        min + (self.rand() % (max - min + 1))
    }

    #[inline]
    pub fn gen_byte(&mut self) -> u8 {
        (self.rand() % 255) as u8
    }

    #[inline]
    pub fn choose<T: PartialOrd + Clone>(&mut self, entries: &[T]) -> T {
        let idx = self.rand() % entries.len();
        entries[idx].clone()
    }

    #[inline]
    pub fn bool(&mut self) -> bool {
        self.choose(&[true, false])
    }

    #[inline]
    pub fn fill_bytes(&mut self, buf: &mut Vec<u8>, sz: usize) {
        while buf.len() < sz {
            buf.extend_from_slice(&self.rand().to_ne_bytes());
        }
    }
}

#[derive(Debug, Clone)]
pub enum Mutator {
    BitFlip,
    ByteFlip,
    NegateByte,
    SwapNeighbors,
    SwapEndianness,
    Arithmetic,
    DeleteBytes,
    DeleteRange,
    CopyBytes,
    CopyRange,
    InsertConstants,
    Truncate,
    Append,
    Set,
    Splice,
    InsertFromDict,
}

#[derive(Debug)]
pub struct MutationEngine {
    pub mutator: Mutator,
    pub test_case: TestCase,
    pub prng: Rng,
    pub mutators: Vec<Mutator>,
    pub token_dict: Option<Vec<String>>,
    pub corpus: Option<Arc<Vec<Vec<u8>>>>,
}

impl MutationEngine {
    pub fn new(
        test_case: Option<TestCase>,
        prng_seed: Option<usize>,
        token_dict: Option<Vec<String>>,
        corpus: Option<Arc<Vec<Vec<u8>>>>,
    ) -> Self {
        let mut mutators = [
            Mutator::BitFlip,
            Mutator::ByteFlip,
            Mutator::NegateByte,
            Mutator::SwapNeighbors,
            Mutator::SwapEndianness,
            Mutator::Arithmetic,
            Mutator::DeleteBytes,
            Mutator::DeleteRange,
            Mutator::CopyRange,
            Mutator::CopyBytes,
            Mutator::InsertConstants,
            Mutator::Truncate,
            Mutator::Append,
            Mutator::Set,
        ]
        .to_vec();
        if token_dict.is_some() {
            mutators.push(Mutator::InsertFromDict);
        }
        if corpus.is_some() {
            mutators.push(Mutator::Splice);
        }
        let mut prng = if let Some(seed) = prng_seed {
            Rng::new(seed)
        } else {
            Rng::new(0)
        };

        let test_case = if let Some(tc) = test_case {
            tc
        } else {
            let mut tc = TestCase::default();
            prng.fill_bytes(&mut tc.data, tc.size);
            tc
        };
        MutationEngine {
            mutator: Mutator::BitFlip,
            test_case,
            prng,
            mutators,
            token_dict,
            corpus,
        }
    }

    #[inline]
    fn mutation_size(&mut self) -> usize {
        let mutation_factor = ((self.prng.gen_range(0, 10) + 1) as f64) * 0.01;
        (self.test_case.size as f64 * mutation_factor) as usize + 1
    }

    fn get_mutator(&mut self, num: usize) {
        self.mutator = match num {
            0 => Mutator::BitFlip,
            1 => Mutator::ByteFlip,
            2 => Mutator::NegateByte,
            3 => Mutator::SwapNeighbors,
            4 => Mutator::SwapEndianness,
            5 => Mutator::Arithmetic,
            6 => Mutator::DeleteBytes,
            7 => Mutator::DeleteRange,
            8 => Mutator::CopyBytes,
            9 => Mutator::CopyRange,
            10 => Mutator::InsertConstants,
            11 => Mutator::Truncate,
            12 => Mutator::Append,
            13 => Mutator::Set,
            14 => Mutator::Splice,
            15 => Mutator::InsertFromDict,
            _ => unreachable!(),
        }
    }

    fn select_random_test_case(&mut self) {
        self.test_case.data.clear();
        if let Some(corp) = &self.corpus {
            assert!(corp.len() > 0, "Corpus does not contain any files.");
            let chosen = &corp[self.prng.rand() % corp.len()];
            self.test_case.data.extend_from_slice(chosen);
            self.test_case.size = chosen.len();
        } else {
            let sz: usize = 4096;
            let mut data = Vec::with_capacity(sz);
            self.prng.fill_bytes(&mut data, sz);
            self.test_case = TestCase::new(&data);
        }
    }

    pub fn mutate(&mut self) -> &Vec<u8> {
        let m = self.prng.gen_range(0, self.mutators.len() - 1);
        self.get_mutator(m);
        debug!("Chosen Mutator: {:#?}", self.mutator);
        self.select_random_test_case();
        match self.mutator {
            Mutator::BitFlip => self.bit_flip(),
            Mutator::ByteFlip => self.byte_flip(),
            Mutator::NegateByte => self.negate_byte(),
            Mutator::SwapNeighbors => self.swap_neighbors(),
            Mutator::SwapEndianness => self.swap_with_width(),
            Mutator::Arithmetic => self.arithmetic(),
            Mutator::DeleteBytes => self.delete_single_bytes(),
            Mutator::DeleteRange => self.delete_byte_range(),
            Mutator::CopyBytes => self.copy_single_bytes(),
            Mutator::CopyRange => self.copy_byte_range(),
            Mutator::InsertConstants => self.insert_constants(),
            Mutator::Truncate => self.truncate(),
            Mutator::Append => self.append(),
            Mutator::Set => self.set(),
            Mutator::Splice => self.splice(),
            Mutator::InsertFromDict => self.insert_from_dict(),
        }
        &self.test_case.data
    }

    fn bit_flip(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_idx = self.prng.gen_range(0, self.test_case.size - 1);
            let rng_byte_pos = self.prng.choose(&BYTE_POS);
            self.test_case.data[rng_idx] ^= rng_byte_pos;
        }
    }

    fn byte_flip(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_idx = self.prng.gen_range(0, self.test_case.size - 1);
            self.test_case.data[rng_idx] ^= self.prng.gen_byte();
        }
    }

    fn set(&mut self) {
        let to_set = self.prng.gen_byte();
        let rng_idx = self.prng.gen_range(0, self.test_case.size - 1);
        let len = self.prng.gen_range(0, (self.test_case.size - rng_idx) - 1);
        self.test_case.data[rng_idx..rng_idx + len]
            .iter_mut()
            .for_each(|x| *x = to_set);
    }

    fn negate_byte(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_idx = self.prng.gen_range(0, self.test_case.size - 1);
            self.test_case.data[rng_idx] = !self.test_case.data[rng_idx];
        }
    }

    fn swap_neighbors(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_idx = self.prng.gen_range(0, self.test_case.size - 2);
            self.test_case.data.swap(rng_idx, rng_idx + 1);
        }
    }

    fn swap_with_width(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_byte_range = self.prng.choose(&BYTE_RANGE) as usize;
            let rng_idx = self.prng.gen_range(0, self.test_case.size - rng_byte_range);
            for i in 0..(rng_byte_range >> 1) {
                let tmp = self.test_case.data[rng_idx + i];
                let swp_idx = rng_byte_range - i - 1;
                self.test_case.data[rng_idx + i] = self.test_case.data[rng_idx + swp_idx];
                self.test_case.data[rng_idx + swp_idx] = tmp
            }
        }
    }

    fn arithmetic(&mut self) {
        for _ in 0..self.mutation_size() {
            let rng_byte_range = self.prng.choose(&BYTE_RANGE) as usize;
            let rng_idx = self.prng.gen_range(0, self.test_case.size - rng_byte_range);
            // TODO measure if it has an impact when making this a bool that flips
            // after each call to have alternate adds/subs
            let op = self.prng.bool();
            match rng_byte_range {
                2 => {
                    let val_vec = &self.test_case.data[rng_idx..rng_idx + rng_byte_range];
                    let mut val: i16 = (val_vec[0] as i16) << 8 | val_vec[1] as i16;
                    if op {
                        val = val.wrapping_add(1);
                    } else {
                        val = val.wrapping_sub(1);
                    }
                    self.test_case.data[rng_idx] = ((val >> 8) & 0xff) as u8;
                    self.test_case.data[rng_idx + 1] = (val & 0xff) as u8;
                }
                4 => {
                    let val_vec = &self.test_case.data[rng_idx..rng_idx + rng_byte_range];
                    let mut val: i32 = (val_vec[0] as i32) << 24
                        | (val_vec[1] as i32) << 16
                        | (val_vec[2] as i32) << 8
                        | (val_vec[3] as i32);
                    if op {
                        val = val.wrapping_add(1);
                    } else {
                        val = val.wrapping_sub(1);
                    }
                    let val_sz = std::mem::size_of_val(&val);
                    for i in 0..val_sz {
                        self.test_case.data[rng_idx + i] =
                            ((val >> (8 * (val_sz - (i + 1)))) & 0xff) as u8;
                    }
                }
                8 => {
                    let val_vec = &self.test_case.data[rng_idx..rng_idx + rng_byte_range];
                    let mut val: i64 = (val_vec[0] as i64) << 56
                        | (val_vec[1] as i64) << 48
                        | (val_vec[2] as i64) << 40
                        | (val_vec[3] as i64) << 32
                        | (val_vec[4] as i64) << 24
                        | (val_vec[5] as i64) << 16
                        | (val_vec[6] as i64) << 8
                        | (val_vec[7] as i64);
                    if op {
                        val = val.wrapping_add(1);
                    } else {
                        val = val.wrapping_sub(1);
                    }
                    let val_sz = std::mem::size_of_val(&val);
                    for i in 0..val_sz {
                        self.test_case.data[rng_idx + i] =
                            ((val >> (8 * (val_sz - (i + 1)))) & 0xff) as u8;
                    }
                }
                _ => {
                    unreachable!()
                }
            };
        }
    }

    fn delete_single_bytes(&mut self) {
        for _ in 0..self.mutation_size() {
            let idx = self.prng.gen_range(0, self.test_case.size - 1);
            self.test_case.data.remove(idx);
            self.test_case.size -= 1;
        }
    }

    fn delete_byte_range(&mut self) {
        let m_sz = self.mutation_size();
        let idx = self.prng.gen_range(0, self.test_case.size - m_sz);
        let _drained: Vec<_> = self.test_case.data.drain(idx..idx + m_sz).collect();
    }

    fn copy_single_bytes(&mut self) {
        for _ in 0..self.mutation_size() {
            let from = self.prng.gen_range(0, self.test_case.size - 1);
            let to = self.prng.gen_range(0, self.test_case.size - 1);
            self.test_case.data[to] = self.test_case.data[from];
        }
    }

    fn copy_byte_range(&mut self) {
        let m_sz = self.mutation_size();
        let from = self.prng.gen_range(0, self.test_case.size - m_sz);
        let to = self.prng.gen_range(0, self.test_case.size - m_sz);
        for i in 0..m_sz {
            self.test_case.data[to + i] = self.test_case.data[from + i];
        }
    }

    fn insert_constants(&mut self) {
        // TODO why 10
        for _ in 0..10 {
            let magic = self.prng.gen_range(0, 4 - 1);
            match magic {
                0 => {
                    let val = self.prng.choose(&MAGIC_8);
                    let to = self.prng.gen_range(0, self.test_case.size - 1);
                    self.test_case.data[to] = val;
                }
                1 => {
                    let val = self.prng.choose(&MAGIC_16);
                    let val_sz = std::mem::size_of_val(&val);
                    let to = self.prng.gen_range(0, self.test_case.size - val_sz);
                    for i in 0..val_sz {
                        self.test_case.data[to + i] =
                            ((val >> (8 * (val_sz - (i + 1)))) & 0xff) as u8;
                    }
                }
                2 => {
                    let val = self.prng.choose(&MAGIC_32);
                    let val_sz = std::mem::size_of_val(&val);
                    let to = self.prng.gen_range(0, self.test_case.size - val_sz);
                    for i in 0..val_sz {
                        self.test_case.data[to + i] =
                            ((val >> (8 * (val_sz - (i + 1)))) & 0xff) as u8;
                    }
                }
                3 => {
                    let val = self.prng.choose(&MAGIC_64);
                    let val_sz = std::mem::size_of_val(&val);
                    let to = self.prng.gen_range(0, self.test_case.size - val_sz);
                    for i in 0..val_sz {
                        self.test_case.data[to + i] =
                            ((val >> (8 * (val_sz - (i + 1)))) & 0xff) as u8;
                    }
                }
                _ => {
                    unreachable!()
                }
            };
        }
    }

    fn truncate(&mut self) {
        let trunc = (self.prng.gen_range(0, 50)) as f64;
        let t = self.test_case.size - self.test_case.size * ((trunc * 0.01) as usize);
        self.test_case.data.truncate(t);
    }

    fn append(&mut self) {
        let m_sz: usize = self.mutation_size();
        let from = self.prng.gen_range(0, self.test_case.size - m_sz);
        let mut slice = vec![0u8; m_sz];
        slice.copy_from_slice(&self.test_case.data[from..from + m_sz]);
        self.test_case.data.append(&mut slice);
        self.test_case.size += m_sz;
    }

    fn splice(&mut self) {
        let split_idx = self.prng.gen_range(0, self.test_case.size - 1);
        let pick = self.prng.rand() % self.corpus.as_ref().unwrap().len();
        let splice_tc = &self.corpus.as_mut().unwrap()[pick];
        let splice_idx = self.prng.gen_range(0, splice_tc.len() - 1);
        self.test_case.data =
            [&self.test_case.data[..split_idx], &splice_tc[splice_idx..]].concat();
    }

    fn insert_from_dict(&mut self) {
        let token_dict = self.token_dict.as_mut().unwrap();
        // TODO why 10
        for _ in 0..10 {
            let pick = self.prng.rand() % token_dict.len();
            let d_ele = &token_dict[pick];
            let d_ele_len = d_ele.len();
            let ele_as_chrs = d_ele.as_bytes();

            let idx = self.prng.gen_range(0, self.test_case.size - d_ele_len);
            self.test_case.data[idx..(d_ele_len + idx)].clone_from_slice(&ele_as_chrs[..d_ele_len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let corpus: Arc<Vec<Vec<u8>>> = Arc::new(
            [
                "ThisIsSomeTest".as_bytes().to_vec(),
                "YetAnotherSimpleInput".as_bytes().to_vec(),
            ]
            .to_vec(),
        );
        let init_tc = TestCase::new(&corpus[0]);
        let mut mutation_engine = MutationEngine::new(Some(init_tc), None, None, Some(corpus));
        let tc = mutation_engine.mutate();
        println!("Mutation: {:?}", String::from_utf8_lossy(&tc.data));

        let expected = "ThisIsSomeTest".to_string();
        let actual = String::from_utf8_lossy(&tc.data);
        assert_ne!(expected, actual);
    }
}
