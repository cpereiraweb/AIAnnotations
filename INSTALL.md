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

Ao abrir pela primeira vez, o app conduz a configuração:

1. Detecta quais CLIs você tem instalados (Claude Code, Codex ou ambos).
2. Orienta a criação dos hooks em cada CLI — veja [guide/](guide/) para os detalhes.
3. Pergunta qual CLI processa os resumos (`claude` ou `codex`). Dá para trocar depois nas configurações.

## Pré-requisitos

- Claude Code e/ou Codex CLI instalados e autenticados.
