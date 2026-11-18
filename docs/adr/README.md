# Decisões de arquitetura (ADRs) do BRDU

Cada decisão de segurança ou produto vira um ADR: contexto, decisão, alternativas e consequências, inclusive os limites honestos. A regra é **nunca decidir sozinho**: o que não foi decidido pelo dono do produto fica `PENDENTE`.

Os ADRs do BRDU nascem na ordem dos marcos; os que vêm do BRDC trazem a linha **Origem**.

| ADR | Título | Status | Origem |
|---|---|---|---|
| [0001](0001-scope-and-goals.md) | Objetivo e escopo do BRDU | Aceito | BRDC 0001 |
| [0002](0002-reserve-backed-by-reais-and-issuers.md) | Lastro em reais e emissores autorizados | Aceito | BRDC 0002 |
| [0003](0003-keys-ed25519-slip10-bip39.md) | Chaves: Ed25519, SLIP-10 e BIP-39 com passphrase | Aceito | BRDC 0005, 0021 |
| [0004](0004-account-model-address-and-decimals.md) | Modelo de conta, endereço e casas decimais | Aceito | BRDC 0010 |
| [0005](0005-deterministic-encoding-and-tagged-hash.md) | Codificação determinística e hash etiquetado | Aceito | BRDC 0011 |
| [0006](0006-no-freezing-uncensored-brdu.md) | Sem congelamento de contas | Aceito, com riscos assumidos | BRDC 0009 |
| [0007](0007-rust-synchronous-std-threads.md) | Rust síncrono, com threads, sem runtime assíncrono | Aceito (contestável com medição) | novo |
| [0008](0008-strict-ed25519-verification.md) | Verificação Ed25519 estrita, e em lote só quando segura | Aceito | novo |
| [0009](0009-fees-in-brdu-table-and-split.md) | Taxas em BRDU: tabela, repartição e ausência de halving | Aceito | BRDC 0008, 0014, 0027 |
| [0010](0010-governance-of-issuers-and-validators.md) | Governança de emissores e validadores | Aceito (testes); real PENDENTE | BRDC 0013 |
| [0017](0017-explorer-verified-replica.md) | Explorador com réplica verificada (não confia no nó) | Aceito | BRDC; ADR-0031 do Teko |
| [0015](0015-validator-rotation.md) | Rotação do conjunto de validadores | Aceito | BRDC 0013; ADR-0045 do Teko |
| [0016](0016-dev-simulators-and-evidence.md) | Simuladores só de dev e evidência sem penalidade | Aceito | BRDC 0028 |
| [0013](0013-noise-transport-bound-to-consensus-key.md) | Transporte Noise XX ligado à chave de consenso | Aceito | BRDC 0007, 0015; `teko-p2p` |
| [0014](0014-node-runtime-gossip-mempool-journal.md) | Runtime do nó: fofoca, mempool O(1), diário e blocos sem vazio | Aceito | BRDC 0026, 0029 |
| [0012](0012-bft-engine-without-io.md) | Motor BFT sem E/S, com as lições do BRDC | Aceito | BRDC 0003, 0026, 0029 |
| [0011](0011-coin-inside-ledger-and-undo-log.md) | A moeda dentro do `brdu-ledger`, e atomicidade por diário de desfazer | Aceito | novo |

## Os ADRs do BRDC e para onde vão
| ADR do BRDC | Destino no BRDU |
|---|---|
| 0003 BFT permissionado, 0016 tempo do bloco e janelas, 0026 diário, 0029 prazo e reenvio | no **M3/M4**, reescritos para o motor em Rust (0029 já vale desde o primeiro dia) |
| 0004 contratos nativos, 0017 catálogo, 0018 bens, 0019 credenciais, 0024 assinatura coletiva | no **M9** |
| 0008 e 0014 taxas, 0027 `signers` e taxa | **feitos no M2** (ADR-0009) |
| 0012 armazenamento | no **M4** |
| 0013 governança de emissores e validadores | **ADR-0010** (M2); a rotação chega no **M4** |
| 0007 P2P com Noise | no **M4**: volta a **Noise** (`snow`), a ideia original; o BRDC usou TLS só porque o Elixir não tinha Noise |
| 0028 laboratório Docker | no **M6** |
| 0006 LiveView, 0015 TLS, 0020 WebCrypto, 0023 nó local com carteira como cliente, 0025 bibliotecas OTP | **não se aplicam** (substituídos por Tauri + daemon e por Noise; ver os ADRs novos dos marcos M4 e M8) |
| 0022 identidade visual | no **M8** (mesmo tema escuro com as cores do Brasil) |

## Decisões PENDENTES (esperam o dono do produto)
| # | Decisão | Quando precisa |
|---|---|---|
| P1 | Quem são os emissores e validadores reais e como o conjunto muda (nas redes de teste: emissores de teste, M de N configurável) | M4 |
