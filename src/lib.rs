#![no_std]

mod catalog;
pub use catalog::*;

pub trait Width: Sized + 'static {}
impl Width for u8 {}
impl Width for u16 {}
impl Width for u32 {}
impl Width for u64 {}

pub struct Algorithm<W: Width> {
    pub poly: W,
    pub init: W,
    pub refin: bool,
    pub refout: bool,
    pub xorout: W,
    pub check: W,
    pub residue: W,
}
