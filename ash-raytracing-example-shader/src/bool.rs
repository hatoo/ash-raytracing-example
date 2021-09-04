use core::ops::Not;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Bool32(u32);

impl Bool32 {
    pub const TRUE: Self = Self(1);
    pub const FALSE: Self = Self(0);
}

impl Bool32 {
    pub fn new(b: bool) -> Self {
        if b {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    pub fn or(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    pub fn and(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl Not for Bool32 {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(1 ^ self.0)
    }
}

impl Into<bool> for Bool32 {
    #[inline]
    fn into(self) -> bool {
        self == Self::TRUE
    }
}

impl From<bool> for Bool32 {
    fn from(b: bool) -> Self {
        Self::new(b)
    }
}
