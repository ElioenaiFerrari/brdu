# ADR-0013 — Transporte Noise XX ligado à chave de consenso

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0007/0015 (lá TLS, por falta de Noise no Elixir) e `teko-p2p` (ADR-0048 do Teko)

## Decisão
- Elo entre validadores = sessão **Noise XX** (`snow`, `25519/ChaChaPoly/SHA256`), com **rekey** em lockstep a cada 65 536 registros.
- A chave estática do Noise (X25519, gerada na 1ª execução, arquivo `noise.key` 0600) é **ligada à chave de consenso Ed25519** por uma assinatura dentro do aperto de mão sobre `{rede, hash da gênese, chave estática, papel}` (`brdu/net/bind`). O papel (1 = quem disca, 2 = quem aceita) impede refletir a prova contra o autor. Rede e gênese diferentes recusam o aperto.
- Só entra quem a **política** aceita: as chaves de consenso do conjunto de validadores (e nunca a própria). Quem disca pode exigir uma instituição específica.
- Limites: aperto de mão inteiro em 5 s (anti slow-loris), tamanho máximo **por tipo de mensagem conferido antes de ler o corpo**, prazo para a mensagem **inteira**, limite de conexões totais, por IP e por chave.
- Registros de endereço assinados (`PeerRecord`/`PeerBook`) existem na crate; a descoberta automática por eles **não está ligada** no nó (os endereços vêm da configuração, M6 liga o compose).

## Consequências e limites honestos
- Sem auditoria. Os endereços são estáticos por enquanto; a reputação (`Reputation`) existe mas o nó só desconecta quem manda lixo.
- Cada par tem **uma** thread dona da sessão (o Noise guarda um contador por direção no mesmo objeto): a latência de envio é a do ciclo de espiada (2 ms sob atividade, 20 ms em repouso).
