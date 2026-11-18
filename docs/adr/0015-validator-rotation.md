# ADR-0015 — Rotação do conjunto de validadores

**Status:** Aceito (2026-11-18, dono do produto escolheu incluir no M5b) · **Origem:** BRDC ADR-0013 e ADR-0045 do Teko

## Contexto
O ADR-0010 dizia que o conjunto de validadores muda por mais de 2/3 do conjunto em vigor. Até o M5 o conjunto era uma constante da gênese: um órgão comprometido só sairia recriando a rede.

## Decisão
- **O conjunto é estado da cadeia:** chaves `val/set` (ordenado: define o rodízio), `val/epoch`, `val/pending`, `val/min` e `val/max` no armazenamento de módulos; entram na raiz de estado. A gênese só dá o conjunto inicial e os limites (`min_validators`, `max_validators ≤ 32`).
- **Transação `VALIDATOR_CHANGE` (tipo 22)**: adicionar ou remover **uma** chave, com altura efetiva. Vale com **mais de 2/3 do conjunto em vigor** assinando o documento `brdu/validators/change` = `{rede, ação, chave, altura efetiva, época atual}` (a época impede reaproveitar aprovação velha). Condições: **uma troca pendente por vez**; altura efetiva **≥ altura do bloco + 2**; tamanho depois da troca entre mín. e máx.; chave nova não é validadora (entrada) / é validadora (saída). Quem envia não autoriza (taxa zero, como mint).
- **Efetivação:** o gancho `Module::end_block` aplica a troca ao fim do bloco `efetiva − 1`; a época sobe. Entrada vai para o **fim** da ordem do rodízio.
- **Cadeia:** proponente, certificado e `signers` do cabeçalho usam o conjunto **em vigor naquela altura**; o certificado do conjunto velho deixa de valer depois da troca.
- **Nó:** o motor de cada altura recebe o conjunto em vigor; quem **não** está nele é **observador** (sincroniza, não vota); a política de conexão é dinâmica (conjunto ∪ chave de entrada pendente, para o novo nó sincronizar antes de valer); na troca de época os pares removidos são derrubados.
- **Simulador:** `cargo dev rotate --in N --out M` (chaves de teste).

## Consequências e limites honestos
- Quem nomeia já está dentro do conjunto: o grupo inicial tende a se perpetuar. Mais de 2/3 em conluio controlam o conjunto; mais de 1/3 podem parar a rede.
- **Sem slashing** (não há stake): equivocação é **detectada e exposta**, não punida (ADR-0016).
- A raiz da cadeia vazia deixou de ser a de um `State` vazio (o conjunto inicial já está no estado); os testes do ledger puro seguem iguais.
- Quais órgãos entram e saem é decisão de implantação, não de código (P1).
