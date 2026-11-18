# ADR-0019 — Credenciais nativas: órgão M-de-N, janela de contestação, validade e revogação

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0019 e ADR-0024 (assinatura coletiva dos órgãos)

## Contexto
Credenciais (CNH, identificação, certidões…) são declarações de um **órgão** sobre uma pessoa. A rede não sabe se são verdadeiras: ela guarda **quem declarou o quê, quando, e se ainda vale**.

## Decisão
1. **Tipo** (tipo de transação 14): criado pelas **autoridades raiz** (M-de-N, definidas na gênese). Fixa o **órgão** (1 a 16 chaves distintas e o limite M), a **janela de contestação** (entre `min_window` e `max_window`), a **validade** (0 = não expira), a **privacidade** e, opcionalmente, um **tipo referenciado**.
2. **Privacidade:** **pública** (dados até 512 bytes na rede) ou **privada** (só um **compromisso** de 32 bytes; quem recebe os dados fora da rede confere contra o compromisso). Privada **não é anônima**: o endereço do titular e o compromisso são públicos.
3. **Emissão** (15): o órgão aprova (M-de-N) um `cred_id` **único para sempre**, o titular, os dados ou compromisso e, se o tipo exige, a **referência** a outra credencial **ativa, do mesmo tipo esperado e do mesmo titular**. Nasce **pendente**.
4. **Janela** (`prazo = hora do bloco + janela`): o **titular** pode **contestar** (16); o **órgão** pode **vetar** (17, M-de-N, com a hora da emissão no documento); depois do prazo **qualquer um ativa** (18).
5. **Revogação** (19): o órgão (M-de-N) revoga uma credencial **pendente ou ativa**, com motivo (até 128 bytes). Definitiva. Contestada e vetada são finais.
6. **Divórcio revoga casamento:** um tipo com `revokes` e referência, ao ser **ativado**, revoga a credencial referenciada.
7. **Validade é lida, não executada:** `valid(id, agora)` = ativa e `agora < expira`; a entrada não muda sozinha.
8. **Taxas:** criar tipo 0,05 BRDU; emitir e revogar 0,01; contestar, vetar e ativar sem mínimo. **A hora é a do bloco em execução** (o ledger passou a posicionar a hora do bloco **antes** de executar as transações, para os módulos).
9. **Compatibilidade:** os documentos que o órgão assina e os payloads têm **o mesmo formato do BRDC**; só o prefixo de domínio muda (`brdu/cred/…`). Com o prefixo do BRDC, os vetores (`issues`, `cred_ops`) são reproduzidos **byte a byte** (teste de oráculo).

## Consequências e limites honestos
- **O órgão é a fonte de verdade:** órgão comprometido emite credenciais falsas ou revoga as verdadeiras; a rede só prova a autoria. A mitigação é o M-de-N e a transparência.
- A rede **não verifica** que casamento e divórcio são do mesmo casal (duas credenciais independentes).
- Sem nome real na rede: a ligação endereço ↔ pessoa é feita pelo órgão fora da rede.
- Autoridades e órgãos de **teste** têm chaves em texto puro (`keys/authority*`, `keys/organ*`). Quem são de verdade é decisão de implantação (P1).
- Fica **planejado**: propostas e aprovações coletivas no nó (ADR-0024 do BRDC) para os membros do órgão assinarem separados; hoje o CLI de desenvolvimento assina com todas as chaves de teste.
