//! Valor em BRDU: inteiro de 64 bits em **unidades mínimas** (ADR-0010). 1 BRDU = 1.000.000 unidades = R$ 1.
//! Nunca usa ponto flutuante.

use alloc::format;
use alloc::string::String;
use core::fmt;

use crate::error::AmountError;

/// Casas decimais.
pub const DECIMALS: usize = 6;
/// Unidades mínimas em 1 BRDU.
pub const UNIT: u64 = 1_000_000;

/// Quantia em unidades mínimas.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Amount(pub u64);

impl Amount {
    /// Zero.
    pub const ZERO: Self = Self(0);

    /// As unidades mínimas.
    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }

    /// Soma, `None` se passar de 64 bits.
    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Subtração, `None` se ficar negativa.
    #[must_use]
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Lê um valor no formato brasileiro: vírgula decimal e ponto **só** como separador de milhar
    /// (`12,5`, `1.000,50`, `0,000001`). Recusa `1.5` (ambíguo), sinais, espaços e mais de 6 casas.
    pub fn parse(text: &str) -> Result<Self, AmountError> {
        let (int_text, frac_text) = match text.split_once(',') {
            Some((i, f)) => (i, Some(f)),
            None => (text, None),
        };
        if frac_text.is_some_and(|f| f.contains(',')) {
            return Err(AmountError::Malformed);
        }
        let int_part = parse_int(int_text)?;
        let frac_part = match frac_text {
            None => 0,
            Some(f) => parse_frac(f)?,
        };
        int_part
            .checked_mul(UNIT)
            .and_then(|v| v.checked_add(frac_part))
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    /// Com as 6 casas: `12.480,570000`.
    #[must_use]
    pub fn to_full(self) -> String {
        let (int, frac) = (self.0 / UNIT, self.0 % UNIT);
        format!("{},{frac:0DECIMALS$}", group_thousands(int))
    }

    /// Sem perder precisão e sem zeros inúteis, mas com ao menos 2 casas: `12.480,57`, `5,00`, `0,001172`.
    #[must_use]
    pub fn to_brl(self) -> String {
        let (int, frac) = (self.0 / UNIT, self.0 % UNIT);
        let digits = format!("{frac:0DECIMALS$}");
        let trimmed = digits.trim_end_matches('0');
        let frac_text = if trimmed.len() < 2 {
            &digits[..2]
        } else {
            trimmed
        };
        format!("{},{frac_text}", group_thousands(int))
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_brl())
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Amount({})", self.0)
    }
}

fn group_thousands(n: u64) -> String {
    let digits = format!("{n}");
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn parse_int(text: &str) -> Result<u64, AmountError> {
    if text.is_empty() {
        return Err(AmountError::Malformed);
    }
    let digits: String = if text.contains('.') {
        // ponto só como milhar: primeiro grupo de 1 a 3 dígitos, os demais com exatamente 3
        let mut groups = text.split('.');
        let first = groups.next().unwrap_or("");
        if first.is_empty() || first.len() > 3 {
            return Err(AmountError::Malformed);
        }
        let mut all = String::from(first);
        for g in groups {
            if g.len() != 3 {
                return Err(AmountError::Malformed);
            }
            all.push_str(g);
        }
        all
    } else {
        String::from(text)
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AmountError::Malformed);
    }
    digits.parse::<u64>().map_err(|_| AmountError::Overflow)
}

fn parse_frac(text: &str) -> Result<u64, AmountError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AmountError::Malformed);
    }
    if text.len() > DECIMALS {
        return Err(AmountError::TooManyDecimals);
    }
    let padded = format!("{text:0<DECIMALS$}");
    padded.parse::<u64>().map_err(|_| AmountError::Malformed)
}
