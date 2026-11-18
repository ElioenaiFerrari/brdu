use thiserror::Error;

/// Erros do transporte.
#[derive(Debug, Error)]
pub enum NetError {
    /// E/S.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// O aperto de mão falhou: mensagem ruim, prova ruim, outra rede ou gênese, chave não aceita.
    #[error("handshake refused: {0}")]
    Handshake(String),
    /// Um quadro não decifrou (adulterado, repetido, fora de ordem).
    #[error("bad frame")]
    BadFrame,
    /// Mensagem maior que o limite do tipo.
    #[error("message too large")]
    TooLarge,
    /// Prazo estourado.
    #[error("timed out")]
    Timeout,
    /// Recusado (limites, política).
    #[error("refused: {0}")]
    Refused(String),
}
