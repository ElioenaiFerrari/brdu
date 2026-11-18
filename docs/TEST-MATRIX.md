# Matriz de testes: invariante → teste

## M1 · cripto e carteira (pronto)
| Invariante | Teste |
|---|---|
| Ed25519 segue o RFC 8032 (vetores 1 e 2); a assinatura é determinística | `crates/brdu-crypto/tests/sign.rs` |
| A verificação é **estrita**: chave inválida, bit trocado na assinatura, outra mensagem e outra chave nunca conferem (propriedades) | `sign.rs` |
| A chave privada nunca aparece em `Debug` | `sign.rs`, `crates/brdu-wallet/tests/wallet.rs` |
| Assinatura com etiqueta só vale com a mesma etiqueta; lote exige **todas** válidas | `sign.rs` |
| **Oráculo do BRDC:** SHA3-256 e hash etiquetado (≥ 20 casos), endereços bech32m e BIP-39 com 3 passphrases batem bit a bit | `crates/brdu-crypto/tests/vectors.rs`, `crates/brdu-wallet/tests/vectors.rs` |
| SLIP-10 para Ed25519: vetor oficial 1 (mestre, `m/0'`, `m/0'/1'`) | `crates/brdu-wallet/tests/vectors.rs` |
| Hash etiquetado sem ambiguidade de fronteira; BRDU e BRDC nunca se confundem | `vectors.rs` |
| Endereço volta igual; prefixo de outra moeda, tamanho errado, lixo e maiúsculas misturadas são recusados | `crates/brdu-crypto/tests/address.rs` |
| **Todo erro de um caractere no endereço é detectado** (propriedade) | `address.rs` |
| Codificação: formato fixo big-endian; ida e volta; truncado, sobra, UTF-8 inválido e tamanho gigante são recusados **sem alocar**; lixo nunca causa pânico | `crates/brdu-crypto/tests/encoding.rs` |
| Valor: `12,5`, `1.000,50`, `0,000001`; `1.5` e ambíguos recusados; limite de 64 bits; escrever e ler volta igual; nunca pânico | `crates/brdu-crypto/tests/amount.rs` |
| Taxa pequena aparece sem perder precisão (`0,001172`) | `amount.rs` |
| Merkle: folha ≠ raiz, ordem importa, tamanhos distintos não colidem, **sem duplicar a última folha** | `crates/brdu-crypto/tests/merkle.rs` |
| Frase: 24 palavras geradas passam no checksum; 12 e 24 aceitas; checksum, contagem e palavra fora da lista recusados; maiúsculas e espaços perdoados | `wallet.rs` |
| **Passphrase diferente = carteira diferente** (propriedade); passphrase inválida (não ASCII, > 64) recusada | `wallet.rs` |
| Caminhos não endurecidos ou malformados recusados; o caminho do BRDU usa a moeda "BRDU" (`1112687701`) | `wallet.rs` |
| Nenhum segredo (palavras, chave) em `Debug` de frase, carteira, conta ou nó | `wallet.rs` |

## M2 · ledger e moeda (pronto)
| Invariante | Teste |
|---|---|
| **Oráculo do BRDC:** as 4 transações do `tx.json` decodificam, reencodificam **byte a byte**, conferem a assinatura (prefixo `brdc/tx`), dão o mesmo id (`brdc/txid`) e a mesma taxa mínima; com os prefixos do BRDU a mesma assinatura **não** vale | `crates/brdu-ledger/tests/tx.rs` |
| A assinatura cobre **todos** os campos (rede, tipo, nonce, taxa, carga, remetente); trocar um bit de qualquer byte nunca vira outra transação válida; lixo nunca causa pânico | `tx.rs` |
| Limites da transação (carga 64 KiB, `chain_id`, tipo 1 a 21), tamanho gigante declarado recusado sem alocar | `tx.rs` |
| Mint: M de N emissores **distintos**; o mesmo emissor repetido conta um; não emissor ignorado; aprovação só vale para o documento exato (valor, referência, rede, conta, tipo); referência uma vez; zero, vazia, grande, estouro | `crates/brdu-ledger/tests/coin.rs` |
| Burn só do dono, com M aprovações; referências de mint e burn separadas | `coin.rs` |
| Atestado: sequência `último + 1`, M aprovações, replay recusado; cobertura (nenhuma, coberta, abaixo) só informativa | `coin.rs` |
| Transferência: taxa mínima exata (1.000 + 1/byte), replay, nonce futuro, rede errada, saldo para valor + taxa desfaz tudo | `coin.rs` |
| Bloco vale por inteiro: uma transação inválida derruba o bloco e o estado **não muda**; raiz, ligação, hora, limites, repetida, signatários, assinatura adulterada | `crates/brdu-ledger/tests/block.rs` |
| A taxa é repartida entre os validadores e o resto vai ao proponente; **nenhuma unidade se perde** | `block.rs` |
| `build_block` não altera o estado; o mesmo bloco dá a mesma raiz em outro nó; raiz do estado depois do bloco = `state_root` do cabeçalho | `block.rs` |
| **Oferta = soma dos saldos, recusada não muda nada, replay do histórico dá o mesmo estado, nonce = transações enviadas, cobertura coerente** (propriedade; também com 500 casos) | `crates/brdu-ledger/tests/invariants.rs` |
| Raiz independe da ordem de montagem; conta zerada não existe; atomicidade aninhada; `speculate` não deixa rastro; limite de 64 bits | `invariants.rs` |

## M3 · consenso BFT (pronto)
| Invariante | Teste |
|---|---|
| **Nunca dois blocos finais na mesma altura**, sob qualquer ordem de entrega, com 0 a 4 mudos/caídos (propriedade, 1.500 casos) | `crates/brdu-bft/tests/sim.rs` |
| Com até f=1 falha de 4, todos os vivos decidem (vivacidade); proponente mudo perde a rodada e a seguinte decide | `sim.rs` |
| 2 caídos de 4: não decide, não diverge; partição 2 a 2 trava e, curada com reenvio, decide | `sim.rs` |
| Todo certificado de commit confere e tem mais de 2/3 | `sim.rs`, `tests/types.rs` |
| Certificado recusa: votante repetido, outra rodada, prevote, voto nulo, assinatura adulterada, votante fora do conjunto, outra rede, sem quórum | `types.rs` |
| Equivocação gera evidência; mensagem inválida (não validador, outra rede, altura, assinatura) é ignorada | `sim.rs` |
| Bloco inválido pelo ledger recebe prevote nulo; proposta de quem não é o proponente ou com hash errado é ignorada | `sim.rs` |
| Restaurar do diário não contradiz o que já foi assinado | `sim.rs` |
| Votos, propostas e certificados: codec ida e volta, lixo nunca causa pânico | `types.rs` |

## M4 · P2P, cadeia e nó (pronto)
| Invariante | Teste |
|---|---|
| Noise XX: só validador do conjunto entra; outra rede, outra gênese, chave fora do conjunto, ou instituição diferente da discada são recusadas dos dois lados | `crates/brdu-p2p/src/tests.rs` |
| A prova de identidade não se empresta a outra chave Noise, nem se forja, nem se reflete (papel) | `brdu-p2p` |
| O texto claro e a chave de consenso nunca aparecem no fio; quadro adulterado, repetido ou fora de ordem nunca decifra | `brdu-p2p` |
| Corpo maior que o limite do tipo é recusado **antes** de ser lido; gotejar bytes é cortado pelo prazo da mensagem inteira; rekey em lockstep | `brdu-p2p` |
| Bytes aleatórios na porta nunca abrem sessão (propriedade); lixo depois do aperto de mão é recusado | `brdu-p2p` |
| Registros de endereço: só o mais novo e válido de cada validador; estranho, falsificado, vencido, endereço inválido ignorados; livro sobrevive a reinício | `brdu-p2p` |
| Gênese ida e volta; qualquer campo muda o hash; duplicado, vazio e limiar inválido recusados | `crates/brdu-chain/tests/chain.rs` |
| Bloco só entra com certificado de mais de 2/3, **proponente da rodada**, signatários = conjunto e execução exata; a réplica chega à mesma raiz | `chain.rs` |
| Certificado fraco, de outro bloco, de outra altura ou de rede estranha é recusado e a cadeia não muda; bloco que mente na raiz é recusado mesmo com certificado válido | `chain.rs` |
| Log só de acrescentar corta o rabo incompleto ou corrompido e continua usável | `chain.rs` |
| 4 validadores (TCP+Noise reais) decidem e chegam ao mesmo estado; mesma transação a dois nós entra uma vez | `crates/brdu-node/tests/cluster.rs` |
| Validador cai e volta: recupera do disco e alcança por sincronização; rede toda reinicia do mesmo ponto | `cluster.rs` |
| **Proponente da vez fora do ar: a rodada muda e a rede decide** | `cluster.rs` |
| Rajada de 300 transações de 20 contas: nada perdido, nada duplicado, fofoca sem eco | `cluster.rs` |
| API: status, conta, oferta, bloco com certificado, transação por id, envio com assinatura adulterada recusado | `cluster.rs` |
| Mempool: duplicata O(1), nonce vencido, buraco promovido, limite em transações **e em bytes**, purge | `crates/brdu-node/tests/mempool.rs` |

## M5b · rotação, simuladores, docs-check e menu (pronto)
| Invariante | Teste |
|---|---|
| Troca de validadores só com **mais de 2/3** do conjunto em vigor; aprovação repetida conta uma, de estranho é ignorada, e só vale para o documento exato (chave, altura efetiva, ação, rede, **época**) | `crates/brdu-ledger/tests/gov.rs` |
| Uma troca pendente por vez; altura efetiva futura; entrada/saída respeitam mín. e máx.; chave já validadora/inexistente recusada; payload malformado recusado | `gov.rs` |
| A troca vale **no bloco certo** (fim de `efetiva − 1`); entrada vai ao fim do rodízio, saída preserva a ordem; replay do histórico dá o mesmo conjunto e a mesma raiz | `gov.rs` |
| **Propriedade:** em qualquer sequência de trocas o conjunto fica em [mín, máx], sem repetição, e a época só sobe 1 por troca | `gov.rs` |
| Depois da troca só vale o conjunto novo: certificado do conjunto velho é recusado; `signers` velhos recusados; removido não conta; réplica refaz o mesmo conjunto | `crates/brdu-chain/tests/chain.rs` |
| Limites do conjunto na gênese incoerentes são recusados | `chain.rs` |
| **5º nó entra por rotação** (recusado antes da troca, sincroniza como observador, depois assina blocos); removido vira observador e a rede segue; troca fraca nunca fica pendente; reinício com troca pendente refaz o mesmo estado | `crates/brdu-node/tests/cluster.rs` |
| **Equivocação detectada por 3 de 3** nós honestos, nomeando o equivocador e os dois blocos | `tools/brdu-sim/tests/equivocation.rs` |
| Transação que sai do mempool por falha devolve o nonce; cabeça morta de fila sai depois de cada bloco | `crates/brdu-node/tests/mempool.rs`, `tools/brdu-dev/tests/cli.rs` |
| O CLI faz o ciclo completo (carteira, fund, send, reserve, burn, supply, blocks, blocks-sim) contra nós de verdade; `init` não recria por cima; `reset` só apaga o que criou | `tools/brdu-dev/tests/cli.rs` |
| `docs-check`: link quebrado, âncora, imagem sem alt, código em linha, crate/ADR/fluxo fora do índice, seção do molde, título em inglês, arquivo de segredo | `tools/brdu-dev/src/docs_check.rs` |
| Todo item do menu vira um comando que a ajuda do `brdu-dev` conhece; a linha de comando leva parâmetros e `--dir` | `tools/brdu-menu/tests/catalog.rs` |

## M7 · explorador (pronto)
| Invariante | Teste |
|---|---|
| O explorador sincroniza de uma rede real, **reverifica** cada bloco e serve visão geral, bloco com certificado, transação, conta (as duas pontas da transferência), paginação e busca | `tools/brdu-explorer/tests/explorer.rs` |
| Busca por altura, id, endereço e hash de bloco; **erro de digitação no endereço** é denunciado (checksum) | `explorer.rs` |
| A página sai com **CSP estrita** (`default-src 'none'`, sem script nem estilo inline); `POST` dá 405; rota desconhecida 404 | `explorer.rs` |
| **Nó que mente é recusado e denunciado:** bloco adulterado, fora de ordem e outra gênese não entram (`rejected_blocks`); o honesto entra inteiro | `explorer.rs` |
| Interface pura: valor em unidades mínimas sem ponto flutuante (64 bits), hashes e endereços encurtados, tempo relativo, efeito de letras (classe do caractere, espaços fixos, termina no texto real) | `tools/brdu-explorer/ui/ui.test.mjs` (via `node --test`, chamado pelo teste Rust) |

## M8 · carteira (pronto)
| Invariante | Teste |
|---|---|
| Cofre: ida e volta; senha errada recusada; **adulterar qualquer campo** (endereço, KDF, sal, nonce, cifrado) quebra a autenticação; parâmetros hostis recusados antes do hash; texto cifrado não contém a frase | `crates/brdu-vault/tests/vault.rs` |
| A frase aparece **uma vez**; o cofre é 0600 e não a contém; `create` nunca sobrescreve | `crates/brdu-walletd/tests/wallet.rs` |
| A **passphrase nunca é guardada**; passphrase errada detectada pelo endereço | `wallet.rs` |
| Importar só com o **endereço confirmado** (`address_mismatch`); frase inválida e checksum ruim recusados; a prévia não guarda nada | `wallet.rs` |
| Exportar a frase e trocar a senha exigem a senha de novo; a senha velha deixa de valer; bloqueada, nada funciona | `wallet.rs` |
| Bloqueio por **inatividade**; **limite de tentativas** com espera que dobra (a senha certa não o fura); vale também para exportar e trocar | `wallet.rs` |
| Contra uma rede real: saldo, prévia (taxa, total, saldo depois), valor e endereço inválidos, saldo que não cobre a taxa, envio de **uso único**, prévia some ao bloquear, histórico pelo explorador | `wallet.rs` |
| A interface JSON roteia, valida parâmetros e **nunca devolve senha ou passphrase** | `wallet.rs`, `tools/brdu-web/tests/web.rs` |
| Daemon: CSP estrita, sem CORS, **todo módulo importado é servido**, caminhos fora da lista dão 404 | `web.rs` |
| Daemon: **Host falso (rebinding), Origin de outro site, `text/plain`/form, método errado, JSON ruim e corpo grande são recusados** | `web.rs` |
| Interface: valores sem ponto flutuante (64 bits), `1.5` recusado, teclado só produz valores válidos (fuzz), erros em pt-BR | `apps/wallet/ui/ui.test.mjs` |
| Interface: fluxos completos com o `app.js` real (criar, enviar com prévia, importar com confirmação, desbloquear com campo limpo, histórico, ajustes, serviço fora do ar) | `apps/wallet/ui/smoke.test.mjs` |

## M9a · credenciais (pronto)
| Invariante | Teste |
|---|---|
| Tipo só com M autoridades (2 de 3) e **para o documento exato**; chaves do órgão não valem; sem autoridades nenhum tipo existe; tipos malformados (id, órgão, limite, janela, referência) e taxa mínima | `crates/brdu-credentials/tests/credentials.rs` |
| Emissão exige M do órgão; nasce **pendente** com `prazo = hora do bloco + janela`; tipo, id, dados (512 bytes, 32 se privada) e referência conferidos; **aprovação presa a cada campo** (id, titular, dado, rede) | `credentials.rs` |
| Titular contesta só dentro da janela; órgão veta só dentro dela, com a hora da emissão no documento; ativar só depois (limite exato); finais não mudam | `credentials.rs` |
| **A janela usa a hora do bloco**, não a do bloco anterior; a hora não volta | `credentials.rs`, `crates/brdu-ledger/tests` |
| Validade lida no limite exato; revogação só de pendente ou ativa, presa ao motivo; id **queimado** para sempre | `credentials.rs` |
| Divórcio referencia casamento **ativo, do mesmo titular e do tipo certo**; ao ativar revoga o casamento; divórcio contestado não toca nele | `credentials.rs` |
| Propriedade: qualquer sequência só faz transições legais, e nenhuma credencial some | `credentials.rs` (também com 300 casos) |
| **Oráculo do BRDC:** documentos de emissão, veto e revogação e os payloads batem **byte a byte** com os vetores; com o prefixo do BRDU os documentos diferem (separação de domínio) | `crates/brdu-credentials/tests/oracle.rs` |
| Vida inteira em **4 validadores reais**: tipo, emissão pendente, ativação após a janela, revogação, mesmo estado em todos | `crates/brdu-node/tests/cluster.rs` |
| `cargo dev cred` percorre a vida da credencial contra nós reais | `tools/brdu-dev/tests/cli.rs` |
| Explorador: credenciais na conta do titular, emissão nas movimentações dele, nome legível do tipo da transação | `tools/brdu-explorer/tests/explorer.rs` |
| Carteira: lista as credenciais emitidas para ela; bloqueada, recusa; cartão no início | `crates/brdu-walletd/tests/wallet.rs`, `apps/wallet/ui/smoke.test.mjs` |

## Planejado por marco
M6 limites do Docker e carga; M9 contratos.

## Limites honestos
Implementações sobre bibliotecas RustCrypto (`ed25519-dalek`, `sha3`, `bip39`, `bech32`), **sem auditoria**. Os vetores do BRDC provam a compatibilidade com o BRDC, não a correção absoluta.
