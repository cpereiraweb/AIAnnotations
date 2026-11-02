# Changelog

Todas as mudanças relevantes do AIAnnotations ficam registradas aqui.

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/), e o projeto usa [versionamento semântico](https://semver.org/lang/pt-BR/).

## [Não lançado]

## [0.1.0] - 2026-11-02

Primeira versão pública, em **beta**: instaladores sem assinatura de código para Windows, macOS (Apple Silicon e Intel) e Linux (`.deb` e `.rpm`).

### Adicionado

- Registro automático das sessões do Claude Code e do Codex por hooks dos próprios CLIs, com o comando `aiannotations` empacotado junto do app.
- Leitura das transcrições e do histórico já existentes, com importação na primeira abertura.
- Resumo de cada execução e briefing diário gerados pelo CLI que você escolher (`claude` ou `codex`), com pausa e aviso quando o CLI precisa de login.
- Tela **Hoje**: briefing, trabalhos em andamento e "Precisa de você" (sessões interrompidas, alterações sem commit e afins).
- Telas **Projetos** e **Sessões**, e busca em todas as sessões com Ctrl+K.
- Ícone na bandeja do sistema, com contadores, pausa dos resumos e acesso aos Ajustes.
- Assistente de primeira abertura: detecção dos CLIs, hooks gravados só depois de mostrar o diff (com cópia de segurança), escolha do processamento, importação do histórico e início com o sistema.
- Tela **Ajustes**: processamento e modelos, hooks (instalar e remover), histórico, idioma (português ou inglês), início com o sistema, pausa e versão do app.
- **Editores e terminais**: detecção por sistema, editor, terminal e shell padrão, apps manuais com `{dir}` e `{cmd}`, "abrir com…" e workspaces `.code-workspace`.
- **▶ retomar**: copia o comando e abre o terminal na pasta da sessão, rodando-o no seu shell com as aspas certas (POSIX, fish, PowerShell ou cmd).

### Ainda não incluído

- Base de conhecimento (`/aisave`, `/aifind`, integração com o Obsidian) e a tela de Retomada.
- Atualização automática e instaladores assinados.

[Não lançado]: https://github.com/cpereiraweb/AIAnnotations/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cpereiraweb/AIAnnotations/releases/tag/v0.1.0
