#![no_std]

pub trait Width {}
impl Width for u16 {}
impl Width for u32 {}
impl Width for u64 {}

pub struct Algorithm<W: Width> {
    pub poly: W,
    pub init: W,
    pub xorout: W,
    pub check: W,
    pub residue: W,
    pub refin: bool,
    pub refout: bool,
}

pub const CRC_16_IBM_3740: Algorithm<u16> = Algorithm {
    poly: 0x1021,
    init: 0xffff,
    xorout: 0x0000,
    check: 0x29b1,
    residue: 0x0000,
    refin: false,
    refout: false,
};

// Aliases
pub const CRC_16_AUTOSAR: Algorithm<u16> = CRC_16_IBM_3740;
