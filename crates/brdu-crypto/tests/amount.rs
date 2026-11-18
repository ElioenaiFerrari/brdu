#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Amount, AmountError};
use proptest::prelude::*;

fn ok(text: &str) -> u64 {
    Amount::parse(text).unwrap().units()
}

#[test]
fn formatos_aceitos() {
    assert_eq!(ok("12"), 12_000_000);
    assert_eq!(ok("12,5"), 12_500_000);
    assert_eq!(ok("1.000,50"), 1_000_500_000);
    assert_eq!(ok("1.234.567,891234"), 1_234_567_891_234);
    assert_eq!(ok("0,000001"), 1);
    assert_eq!(ok("0"), 0);
    assert_eq!(ok("007"), 7_000_000);
}

#[test]
fn ambiguos_e_malformados_sao_recusados() {
    // o ponto só vale como milhar: `1.5` NUNCA vira um e meio (nem quinze)
    for bad in [
        "1.5", "1.50", "1.5000", ".5", "1.", "1..000", "1.00", "1234.567", "", ",", ",5", "1,",
        "1,2,3", "+1", "-1", " 1", "1 ", "1e3", "0x10", "R$ 1", "1,5a", "a",
    ] {
        assert_eq!(Amount::parse(bad), Err(AmountError::Malformed), "{bad:?}");
    }
    assert_eq!(
        Amount::parse("1,1234567"),
        Err(AmountError::TooManyDecimals)
    );
}

#[test]
fn limite_de_64_bits() {
    assert_eq!(ok("18.446.744.073.709,551615"), u64::MAX);
    assert_eq!(
        Amount::parse("18.446.744.073.709,551616"),
        Err(AmountError::Overflow)
    );
    assert_eq!(
        Amount::parse("99.999.999.999.999.999.999"),
        Err(AmountError::Overflow)
    );
}

#[test]
fn formatacao() {
    assert_eq!(Amount(12_480_570_000).to_full(), "12.480,570000");
    assert_eq!(Amount(12_480_570_000).to_brl(), "12.480,57");
    assert_eq!(Amount(5_000_000).to_brl(), "5,00");
    assert_eq!(Amount(1_234_500_000).to_brl(), "1.234,50");
    assert_eq!(Amount(1_172).to_brl(), "0,001172"); // taxa: sem perder precisão
    assert_eq!(Amount(1).to_brl(), "0,000001");
    assert_eq!(Amount(0).to_brl(), "0,00");
    assert_eq!(Amount(0).to_full(), "0,000000");
    assert_eq!(Amount(1_000_000_000).to_brl(), "1.000,00");
}

#[test]
fn aritmetica_checada() {
    assert_eq!(Amount(u64::MAX).checked_add(Amount(1)), None);
    assert_eq!(Amount(1).checked_sub(Amount(2)), None);
    assert_eq!(Amount(5).checked_sub(Amount(2)), Some(Amount(3)));
}

proptest! {
    #[test]
    fn escrever_e_ler_de_volta_sempre_fecha(units in any::<u64>()) {
        let a = Amount(units);
        prop_assert_eq!(Amount::parse(&a.to_full()), Ok(a));
        prop_assert_eq!(Amount::parse(&a.to_brl()), Ok(a));
    }

    // o parser nunca entra em pânico e só aceita o que o formatador consegue reproduzir
    #[test]
    fn texto_qualquer_nunca_causa_panico(text in ".{0,40}") {
        if let Ok(a) = Amount::parse(&text) {
            prop_assert_eq!(Amount::parse(&a.to_full()), Ok(a));
        }
    }
}
