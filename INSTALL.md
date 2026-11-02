# Instalação

> Ainda não há versão publicada. Este documento descreve o fluxo previsto e será completado a cada release.

## 1. Baixe o instalador

Na página de [Releases](../../releases), baixe o arquivo do seu sistema:

| Sistema | Arquivo |
|---------|---------|
| Windows | `.msi` ou `.exe` |
| macOS | `.dmg` |
| Linux | `.AppImage` ou `.deb` |

## 2. Libere o app na primeira abertura

Os instaladores ainda não são assinados digitalmente, então o sistema pede uma confirmação na primeira vez:

- **macOS:** clique com o botão direito no app → **Abrir** → **Abrir**.
- **Windows:** na tela do SmartScreen, clique em **Mais informações** → **Executar assim mesmo**.
- **Linux (AppImage):** dê permissão de execução ao arquivo (`chmod +x`).

## 3. Configure na primeira execução

Ao abrir pela primeira vez, o app mostra um assistente em 5 passos. Nenhuma chamada à IA acontece antes de você concluir.

1. **Detecção:** mostra quais CLIs estão instalados (Claude Code, Codex ou ambos).
2. **Hooks:** mostra o diff do que será gravado em `~/.claude/settings.json` e em `~/.codex/hooks.json`. Nada é gravado sem um clique seu, e antes de cada gravação o app faz uma cópia de segurança do arquivo. Hooks de outras ferramentas continuam no arquivo. No Codex, abra `/hooks` depois de gravar e confie nos hooks do AIAnnotations; sem isso, eles não rodam.
3. **Processamento:** escolha qual CLI gera os resumos e o briefing, e os modelos de cada um.
4. **Histórico:** importe as sessões que você já tem. Resumir esse histórico com IA é opcional e usa a cota do CLI escolhido.
5. **Iniciar com o sistema:** liga ou desliga a abertura automática.

## 4. Ajustes

Para mudar essas escolhas depois, use **⚙ Ajustes**, na tela principal ou no menu do ícone da bandeja. Cada mudança é salva na hora, sem botão Salvar.

- **Processamento, Hooks e Histórico:** as mesmas opções do assistente. Em Hooks também dá para **remover os hooks**: o app mostra o diff do que sai e só remove depois da sua confirmação.
- **Idioma:** Automático (segue o idioma do sistema), Português ou English. A troca vale na hora para as telas, o menu da bandeja, os resumos e o briefing.
- **Pausar resumos e briefing:** suspende as chamadas à IA. É a mesma opção do menu da bandeja.
- **Editores e terminais:** o editor, o terminal e o shell padrão, uma pasta de workspaces e apps manuais. Veja o [guia](guide/editores-e-terminais.md).
- **Refazer a primeira execução:** abre o assistente de novo.

Se a IA falhar, por exemplo porque o CLI precisa de um novo login, a tela Hoje mostra o motivo.

## Pré-requisitos

- Claude Code e/ou Codex CLI instalados e autenticados.
