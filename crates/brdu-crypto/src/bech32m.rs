//! bech32m (BIP-350) com prefixo legível. O checksum detecta erro de digitação.

use alloc::string::String;
use alloc::vec::Vec;
use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};

use crate::error::AddressError;

/// Codifica `data` com o prefixo `hrp` (ex.: `brdu`).
pub fn encode(hrp: &str, data: &[u8]) -> Result<String, AddressError> {
    let hrp = Hrp::parse(hrp).map_err(|_| AddressError::Malformed)?;
    bech32::encode::<Bech32m>(hrp, data).map_err(|_| AddressError::Malformed)
}

/// Decodifica e devolve `(prefixo, dados)`. Recusa bech32 antigo (só aceita bech32m) e checksum errado.
pub fn decode(text: &str) -> Result<(String, Vec<u8>), AddressError> {
    let checked = CheckedHrpstring::new::<Bech32m>(text).map_err(|_| AddressError::Malformed)?;
    let hrp = String::from(checked.hrp().as_str());
    let data: Vec<u8> = checked.byte_iter().collect();
    Ok((hrp, data))
}
