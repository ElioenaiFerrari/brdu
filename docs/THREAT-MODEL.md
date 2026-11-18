# Modelo de ameaças

Começa com o que o M1 cobre e cresce a cada marco. Cada linha diz o que é mitigado e o que **não** é.

| # | Ameaça | Mitigação | Limite honesto |
|---|---|---|---|
| T1 | Frase ou chave vazada (log, `Debug`, serialização) | tipos de segredo sem `Debug`/`Serialize`/`Clone`; `zeroize` ao sair da memória; mensagens de erro sem segredo; testes conferem `Debug` | o sistema operacional pode paginar memória; malware na máquina vê tudo |
| T2 | Passphrase errada ou esquecida: abre **outra carteira válida e vazia**, sem erro | a importação mostra o endereço derivado e pede confirmação (marco M8); o doc avisa | **não há como validar** a passphrase; esquecê-la perde os fundos |
| T3 | Erro de digitação no endereço (fundos para lugar nenhum) | bech32m: **todo erro de um caractere é detectado** (propriedade testada) | colar o endereço de outra pessoa não é erro de digitação |
| T4 | Um hash aceito como outro (transação como bloco) | prefixo de domínio no hash (`brdu/tx`, `brdu/block`…); BRDU e BRDC não se confundem | — |
| T5 | Maleabilidade de assinatura (duas assinaturas válidas para a mesma mensagem) | verificação **estrita**: recusa assinaturas não canônicas e chaves de ordem pequena | a verificação em lote do dalek não é estrita (ver ADR-0008) |
| T6 | Ambiguidade de Merkle (duplicar a última folha; folha passando por nó) | domínios separados para folha e nó; divisão estilo RFC 6962 | — |
| T7 | Decodificador alocando memória gigante por um tamanho mentiroso | todo campo variável tem limite e é conferido **antes** de alocar; contagem maior que os bytes restantes é recusada | os limites concretos de cada mensagem vêm com o protocolo (M4) |
| T8 | Valor lido errado (`1.5` virando 15) | o ponto só vale como milhar; ambíguos recusados; inteiros de 64 bits, sem ponto flutuante | — |
| T9 | Dependência comprometida (cadeia de suprimentos) | poucas dependências, todas RustCrypto ou amplamente usadas; `Cargo.lock` versionado | **sem auditoria nem `cargo deny` rodando ainda** (planejado no M10) |

| T10 | **Mint sem lastro:** menos de M emissores aprovando, o mesmo emissor várias vezes, aprovação reaproveitada para outro valor ou conta | o ledger conta só aprovações **válidas e de emissores distintos** sobre o **documento exato** (valor, referência, rede, conta, tipo); referência única | M emissores em conluio criam moeda sem lastro; a rede só prova que **assinaram**, não que os reais existem |
| T11 | Replay da transação (na mesma rede ou em outra) | nonce por conta e `chain_id` dentro da assinatura; os documentos dos emissores também carregam a rede | — |
| T12 | Bloco aplicado pela metade (estado inconsistente entre nós) | bloco **inteiro ou nada** (diário de desfazer); raiz de estado conferida contra o cabeçalho; determinismo testado por replay | a raiz é recalculada por inteiro a cada bloco (custo, não segurança) |
| T13 | Estouro aritmético (criar moeda do nada) | toda soma e subtração é checada; estouro desfaz tudo (teste com `u64::MAX`) | — |
| T14 | Taxa que some ou aparece (quebra a oferta) | a taxa é repartida entre os validadores e o resto vai ao proponente: `Σ saldos = emitido − queimado` (propriedade) | no estado pendente do mempool a taxa só sai da conta |
| T15 | Burn do saldo alheio | só o titular queima; o remetente tem de ser a conta | — |

| T16 | Estranho se passa por validador na rede | Noise XX com prova Ed25519 ligada à chave estática, rede e gênese; política = conjunto de validadores | validadores comprometidos (mais de 1/3) fora da garantia |
| T17 | Proponente propõe bloco que mente (raiz, signatários ou proponente falsos) | validadores ensaiam o bloco inteiro antes de votar; a réplica confere certificado, proponente da rodada e signatários | — |
| T18 | Validador contradiz o próprio voto depois de uma queda | diário gravado (`fsync`) antes de transmitir; `restore` não assina o que contradiz | disco que mente sobre o `fsync` |
| T19 | Spam de transações / mempool enchendo a memória | taxa mínima, nonce contíguo, limite em transações e em bytes, dedupe O(1), tamanho por tipo antes de ler | taxa é barata (R$ 0,0012): o limite de bytes é a defesa real |
| T20 | Par lento ou malicioso trava a rede | fila de saída limitada (perde a mensagem, a sincronização repõe), prazos por mensagem, limites de conexão | sem reputação ligada ao nó ainda |

| T21 | Troca de validadores sem aval (entrada de um aliado, saída de um rival) | mais de 2/3 do conjunto em vigor sobre o documento exato com a **época**; uma pendente; altura futura; limites mín./máx. | mais de 2/3 em conluio controlam o conjunto; quem nomeia já está dentro |
| T22 | Replay de aprovação de uma troca antiga | o documento carrega a época; mudou a época, a aprovação não vale | — |
| T23 | Certificado do conjunto antigo aceito depois da troca | o `Chain` verifica cada bloco contra o conjunto **daquela** altura | — |
| T24 | Validador equivoca (vota em dois blocos) | evidência assinada exposta em `/evidence` (3 de 3 nos testes) | sem stake não há penalidade: a resposta é a rotação |
| T25 | Nó que saiu (ou ainda não entrou) abre conexões | política de conexão dinâmica: conjunto ∪ chave de entrada pendente; pares removidos são derrubados | a chave que está para entrar pode conectar como observador antes de valer |

| T26 | Roubo do arquivo do cofre | Argon2id (64 MiB, 3 passes) + ChaCha20-Poly1305; sem a senha só resta força bruta | senha fraca cai; sem auditoria |
| T27 | Força bruta na senha pela interface | limite de tentativas com espera que dobra (até 60 s), também ao exportar a frase e trocar a senha | quem tem o arquivo ataca offline (Argon2id encarece) |
| T28 | Página de outro site (ou rebinding de DNS) usa o daemon web | só loopback, `Host` e `Origin` conferidos, `/rpc` só `application/json`, sem CORS, CSP estrita | as chaves vivem no processo do daemon: **só chaves de teste** |
| T29 | Endereço com erro de digitação ou colado errado | checksum bech32m recusa; a prévia mostra o endereço completo e a tela de importar exige confirmá-lo | trocar um endereço válido por outro válido (malware na área de transferência) não é detectável |
| T30 | Enviar mais do que tem, ou sem cobrir a taxa | `prepare_send` confere valor + taxa; a rede confere de novo | — |
| T31 | Frase espiada na tela | mostrada uma vez (e de novo só com a senha), campos limpos após o uso | quem vê a tela vê a frase |

| T32 | Credencial falsa emitida sem o órgão | M aprovações **distintas** do órgão do tipo sobre o documento exato (rede, tipo, id, titular, dados, referência) | órgão comprometido (M chaves) emite o que quiser |
| T33 | Aprovação do órgão reaproveitada para outra credencial, outro titular, outra rede ou outro dado | o documento inclui todos esses campos; o veto inclui a hora da emissão; a revogação inclui o motivo | — |
| T34 | Tipo criado sem aval (um órgão falso) | só as autoridades raiz da gênese (M-de-N) criam tipos | autoridades em conluio |
| T35 | Reemitir um id contestado, vetado ou revogado | o `cred_id` fica queimado para sempre | — |
| T36 | Vazar dados pessoais na rede | credencial **privada**: só um compromisso de 32 bytes; o titular revela fora da rede | o endereço do titular e o fato de ter a credencial são públicos |

| T37 | Bem falso registrado sem o órgão | M aprovações distintas do órgão da classe sobre o documento exato | órgão comprometido registra o que quiser |
| T38 | Aprovação de veto ou gravame reaproveitada | o documento inclui a revisão do bem (e comprador, preço e prazo no veto) | — |
| T39 | Vendedor leva o dinheiro e não entrega (ou o contrário) | escrow: o preço sai do comprador no aceite e só vai ao vendedor junto com a propriedade | o dinheiro fica preso durante a janela |
| T40 | Finalizar com gravame para fugir de dívida | finalização falha enquanto houver gravame | órgão que não remove o gravame |

| T41 | Contrato que cria ou destrói dinheiro (estouro, custódia errada) | aritmética em 128 bits com recusa de estouro; custódia contabilizada e testada por propriedade | — |
| T42 | Parte errada ou fora do prazo aciona uma transição | estado, ator e condições de tempo conferidos pelo ledger com a hora do bloco | — |
| T43 | Código arbitrário em contrato | não há VM: vocabulário fechado, modelos revisados no código | modelo novo exige atualização da rede |

Os itens do BRDC (T10 a T29: equivocação, mempool, laboratório, órgãos, contratos, explorador mentiroso…) entram com os marcos que os criam, reescritos para o Rust.
