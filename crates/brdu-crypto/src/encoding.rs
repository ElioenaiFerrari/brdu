//! Codificação determinística (ADR-0011): campos em ordem fixa, inteiros big-endian, tamanho de 4 bytes antes de
//! cada campo variável. Nunca há dois jeitos de escrever o mesmo valor, então o hash é estável entre máquinas.

use alloc::vec::Vec;

use crate::error::DecodeError;

/// Monta bytes campo a campo.
#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
    overflow: bool,
}

impl Encoder {
    /// Um codificador vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Um byte.
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    /// Inteiro de 32 bits, big-endian.
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Inteiro de 64 bits, big-endian.
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Bytes com o tamanho (4 bytes) na frente.
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        match u32::try_from(v.len()) {
            Ok(n) => {
                self.u32(n);
                self.buf.extend_from_slice(v);
            }
            Err(_) => self.overflow = true,
        }
        self
    }

    /// Texto UTF-8 com o tamanho na frente.
    pub fn string(&mut self, v: &str) -> &mut Self {
        self.bytes(v.as_bytes())
    }

    /// Bytes de tamanho fixo e conhecido de quem lê, sem prefixo (chaves, hashes, assinaturas).
    pub fn fixed(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }

    /// Quantidade de itens de uma lista (4 bytes); os itens vêm depois, um a um.
    pub fn count(&mut self, n: usize) -> &mut Self {
        match u32::try_from(n) {
            Ok(n) => {
                self.u32(n);
            }
            Err(_) => self.overflow = true,
        }
        self
    }

    /// Termina. Falha só se algum campo passou de 4 GiB (nunca acontece com mensagens de verdade).
    pub fn finish(self) -> Result<Vec<u8>, DecodeError> {
        if self.overflow {
            Err(DecodeError::TooLong)
        } else {
            Ok(self.buf)
        }
    }
}

/// Lê bytes campo a campo, na mesma ordem em que o [`Encoder`] escreveu.
#[derive(Debug, Clone, Copy)]
pub struct Decoder<'a> {
    data: &'a [u8],
}

impl<'a> Decoder<'a> {
    /// Começa a ler `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let (head, rest) = self
            .data
            .split_at_checked(n)
            .ok_or(DecodeError::Truncated)?;
        self.data = rest;
        Ok(head)
    }

    /// Um byte.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        self.take(1)?.first().copied().ok_or(DecodeError::Truncated)
    }

    /// Inteiro de 32 bits.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(self.fixed::<4>()?))
    }

    /// Inteiro de 64 bits.
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_be_bytes(self.fixed::<8>()?))
    }

    /// Exatamente `N` bytes.
    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.take(N)?.try_into().map_err(|_| DecodeError::Truncated)
    }

    /// Bytes com tamanho na frente; recusa mais de `max` (não aloca nada antes de conferir).
    pub fn bytes(&mut self, max: usize) -> Result<&'a [u8], DecodeError> {
        let n = usize::try_from(self.u32()?).map_err(|_| DecodeError::TooLong)?;
        if n > max {
            return Err(DecodeError::TooLong);
        }
        self.take(n)
    }

    /// Texto UTF-8 com tamanho na frente; recusa mais de `max` bytes.
    pub fn string(&mut self, max: usize) -> Result<&'a str, DecodeError> {
        core::str::from_utf8(self.bytes(max)?).map_err(|_| DecodeError::Utf8)
    }

    /// Quantidade de itens de uma lista; recusa mais de `max` (cada item ocupa ao menos 1 byte, então uma
    /// contagem maior que os bytes que sobram é truncada).
    pub fn count(&mut self, max: usize) -> Result<usize, DecodeError> {
        let n = usize::try_from(self.u32()?).map_err(|_| DecodeError::TooLong)?;
        if n > max {
            return Err(DecodeError::TooLong);
        }
        if n > self.data.len() {
            return Err(DecodeError::Truncated);
        }
        Ok(n)
    }

    /// Quantos bytes ainda não foram lidos.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len()
    }

    /// Confere que não sobrou nada.
    pub fn finish(self) -> Result<(), DecodeError> {
        if self.data.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::Trailing)
        }
    }
}
