//! Erros das primitivas. Nenhuma mensagem de erro carrega segredo.

use thiserror::Error;

/// Falhas de criptografia (assinatura, chave).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CryptoError {
    /// A assinatura não confere com a chave e a mensagem (ou é malformada/não canônica).
    #[error("assinatura inválida")]
    BadSignature,
    /// Os bytes não são uma chave pública válida.
    #[error("chave pública inválida")]
    BadKey,
}

/// Falhas ao ler um endereço `brdu1…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AddressError {
    /// Não é bech32m válido (checksum errado, caractere fora do alfabeto, maiúsculas e minúsculas misturadas…).
    #[error("endereço malformado ou com erro de digitação")]
    Malformed,
    /// É bech32m válido, mas de outra moeda (prefixo diferente).
    #[error("prefixo de endereço diferente do esperado")]
    WrongPrefix,
    /// O conteúdo não tem 32 bytes.
    #[error("tamanho de endereço inválido")]
    BadSize,
}

/// Falhas ao ler bytes codificados de forma determinística.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// Acabaram os bytes antes do fim do campo.
    #[error("dados truncados")]
    Truncated,
    /// Sobraram bytes depois do último campo.
    #[error("sobraram bytes depois do fim")]
    Trailing,
    /// O campo de texto não é UTF-8 válido.
    #[error("texto não é UTF-8 válido")]
    Utf8,
    /// O tamanho declarado passa do limite aceito (proteção contra alocação gigante).
    #[error("campo maior que o limite")]
    TooLong,
}

/// Falhas ao ler ou montar um valor em BRDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AmountError {
    /// Texto vazio ou com caractere que não faz parte de um número.
    #[error("valor malformado")]
    Malformed,
    /// Mais de 6 casas decimais.
    #[error("no máximo 6 casas decimais")]
    TooManyDecimals,
    /// O valor não cabe em 64 bits.
    #[error("valor grande demais")]
    Overflow,
}
