# Editores e terminais

O app abre a pasta de um projeto no seu editor e retoma uma sessão no seu terminal. Esta página explica o que ele detecta, como escolher os padrões e como cadastrar um app que ele não conhece.

As opções ficam em **Ajustes**, na seção **Editores e terminais**. Cada mudança é salva na hora.

## O que o app detecta

A detecção roda na abertura do app e de novo quando você clica em **Detectar de novo**. Ela procura no `PATH` e nos locais de instalação de cada sistema.

**Editores** (nos três sistemas):

- Visual Studio Code, Cursor, Windsurf, Antigravity
- Zed, Sublime Text
- PhpStorm, IntelliJ IDEA, WebStorm, PyCharm, GoLand, Rider, CLion, RubyMine, RustRover
- O gerenciador de arquivos do sistema, que sempre existe

**Terminais:**

| Sistema | Terminais detectados |
|---------|----------------------|
| Linux | kitty, Ghostty, WezTerm, Alacritty, Ptyxis, GNOME Terminal, Konsole e o `x-terminal-emulator` do sistema |
| macOS | Terminal, iTerm2, kitty, Ghostty, WezTerm, Alacritty |
| Windows | Windows Terminal, PowerShell, Command Prompt, Cmder, WezTerm, Alacritty |

Só aparece o que está instalado. Se você instalou algo com o app aberto, use **Detectar de novo**.

## Escolher os padrões

Os três seletores têm a opção **Automático (nome)**, que usa o primeiro app encontrado na ordem de preferência do app e mostra qual é.

- **Editor padrão:** o app do botão **Abrir no \<editor\>**.
- **Terminal padrão:** o terminal em que **▶ retomar** roda o comando. Se nenhum for encontrado, o seletor mostra "Nenhum terminal encontrado".
- **Shell:** o shell em que o comando de retomada roda, por exemplo bash, zsh, fish, PowerShell ou Git Bash. O comando é escrito com as aspas desse shell. Nos terminais que são o próprio shell (PowerShell e Command Prompt), vale o shell do terminal, não o configurado.

## Abrir um projeto

O botão **Abrir no \<editor\>** aparece na tela Projetos, no cabeçalho de um projeto e nos grupos da caixa de entrada e dos trabalhos em andamento. Ao lado dele, **abrir com…** mostra a lista de **Editores** e **Terminais** encontrados (mais os apps manuais). Escolher um editor abre a pasta nele. Escolher um terminal abre o terminal na pasta, sem rodar comando.

## Pasta de workspaces

Se você preencher **Pasta de workspaces (.code-workspace)**, o app procura nela um arquivo de workspace do projeto antes de abrir a pasta. Deixar o campo vazio desliga o recurso.

Vale só para os editores da família do VS Code: Visual Studio Code, Cursor, Windsurf e Antigravity. Os outros sempre abrem a pasta. A escolha do arquivo segue esta ordem:

1. um workspace com uma única pasta, que é a do projeto;
2. o que tem o mesmo nome da pasta do projeto;
3. o primeiro em ordem alfabética.

Se nenhum arquivo servir, o editor abre a pasta normalmente.

## Retomar uma sessão

**▶ retomar** (ou **▶ retomar (claude)**, **▶ retomar (codex)**) faz duas coisas: copia o comando de retomada para a área de transferência e abre o terminal padrão na pasta do projeto, rodando o comando no shell configurado. O terminal continua aberto depois que o comando termina.

O aviso que aparece depois diz o que aconteceu:

| Aviso | Significado |
|-------|-------------|
| Comando copiado e terminal aberto. | Deu certo: o terminal abriu e já está rodando o comando. |
| Terminal aberto. Cole o comando copiado. | O terminal abriu na pasta, mas não recebeu o comando (acontece com apps manuais sem `{cmd}`, ou com terminais que não aceitam comando). Cole com Ctrl+V. |
| Comando copiado. | Não há terminal para abrir. Cole o comando onde quiser. |
| Comando copiado, mas o terminal não abriu: _motivo_ | O comando está na área de transferência, mas o terminal falhou ao abrir. O motivo vem no aviso. |

Avisos de erro ao salvar ou abrir um app manual:

- **O modelo de argumentos é inválido:** use `{dir}` para a pasta (e `{cmd}` só em terminais). Aspas sem fechar também causam esse erro.
- **O executável não foi encontrado:** use o caminho absoluto de um arquivo que existe.
- **O app não foi encontrado:** o app escolhido não existe mais. Use **Detectar de novo**.
- **O arquivo de ongoing é inválido:** o arquivo do trabalho em andamento tem um nome que o app não aceita.

## Cadastrar um app manual

Em **Apps manuais**, preencha:

- **Tipo:** Editor ou Terminal;
- **Nome:** como ele aparece nas listas;
- **Executável:** o caminho absoluto do programa;
- **Argumentos:** o modelo da linha de comando.

O app roda o executável direto, sem passar por `sh -c`. Por isso as regras do modelo são estas:

- O modelo é dividido em argumentos como um shell faria, respeitando aspas simples e duplas.
- `{dir}` é trocado pela pasta do projeto em qualquer lugar do argumento, inclusive no meio de um texto (`--cwd={dir}`).
- Um argumento que é exatamente `{cmd}` vira a chamada do shell com o comando de retomada, em vários argumentos.
- `{cmd}` dentro de um argumento maior (`"bash -c {cmd}"`) vira só o texto do comando.
- Um terminal sem `{cmd}` no modelo só abre na pasta. Ao retomar, o app avisa "Terminal aberto. Cole o comando copiado."
- Um editor não pode usar `{cmd}`.
- Modelo vazio: um editor recebe `{dir}`; um terminal é tratado como um terminal genérico (`-e <comando>`).

Exemplos:

1. **Um terminal do Linux que não está na lista**, o `foot`, que abre na pasta com `-D` e roda o programa que vem depois das opções:

   - Tipo: Terminal
   - Executável: `/usr/bin/foot`
   - Argumentos: `-D {dir} {cmd}`

   Com **▶ retomar**, o `{cmd}` vira as palavras do shell que roda o comando e mantém o terminal aberto.

2. **O mesmo terminal, só abrindo na pasta**, sem rodar comando:

   - Argumentos: `-D {dir}`

3. **Um editor com uma opção a mais**, o VS Code abrindo em uma janela nova:

   - Tipo: Editor
   - Executável: `/usr/bin/code`
   - Argumentos: `--new-window {dir}`

Um app manual não usa a pasta de workspaces: ela vale só para os editores da família do VS Code que o app detecta.
