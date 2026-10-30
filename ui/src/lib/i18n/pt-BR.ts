// Textos do v1 (Inv §1). Mesmas chaves de en.ts; o tipo garante a paridade em tempo de checagem.
import type { Entry, Key } from './en';

export const ptBR: Record<Key, Entry> = {
  // Layout
  'nav.today': '☀ Hoje',
  'nav.projects': '▦ Projetos',
  'nav.sessions': '☰ Sessões',
  'nav.main': 'Navegação principal',
  'nav.search': 'buscar',

  // Toasts e erros
  'toast.copied': 'comando copiado',
  'toast.copy_failed': 'não foi possível copiar o comando',
  'toast.briefing_running': 'O briefing já está sendo gerado.',
  'toast.import_running': 'O histórico já está sendo importado.',
  'toast.import_started': 'Importando o histórico…',
  'error.generic': 'Algo deu errado: {message}',

  // Hoje: briefing
  'briefing.title': 'Briefing de {date}',
  'briefing.generating': 'gerando…',
  'briefing.failed': 'Não foi possível gerar o briefing agora.',
  'briefing.regenerate': 'regerar',

  // Hoje: ongoings
  'ongoing.title': 'Em andamento · {count}',
  'ongoing.none_recent': 'Nenhum ongoing alterado nos últimos {days} dias.',
  'ongoing.stale': 'Parados há mais de {days} dias · {count}',

  // Hoje: inbox
  'inbox.title': 'Precisa de você · {count}',
  'inbox.empty': 'Nada pendente. Bom trabalho.',
  'inbox.older': 'Parado há mais tempo · {count}',
  'inbox.dismiss': 'já vi',
  'inbox.dismiss_all': 'já vi tudo',
  'inbox.ahead': { one: '{count} commit à frente, sem merge', other: '{count} commits à frente, sem merge' },

  // Hoje: próximos passos, backfill, base vazia
  'today.next_steps': 'Próximos passos sugeridos',
  'today.backfill': 'histórico: {done}/{total} resumidas',
  'today.empty_db': 'Nenhuma sessão ainda.',
  'today.import': 'Importar histórico',

  // Badges do inbox
  'badge.interrupted': 'interrompida',
  'badge.half_done': 'pela metade',
  'badge.stale_branch': { one: 'sem merge há {count} dia', other: 'sem merge há {count} dias' },
  'badge.uncommitted_changes': 'alterações não commitadas',
  'badge.worktree': 'worktree parada',
  'badge.reminder': 'lembrete',
  'badge.ongoing': 'retomada',

  // Contadores do grupo, na ordem de CountKey
  'counts.interrupted': { one: '{count} interrompida', other: '{count} interrompidas' },
  'counts.half_done': { one: '{count} pela metade', other: '{count} pela metade' },
  'counts.ongoing': { one: '{count} retomada', other: '{count} retomadas' },
  'counts.reminder': { one: '{count} lembrete', other: '{count} lembretes' },
  'counts.stale_branch': { one: '{count} branch', other: '{count} branches' },
  'counts.worktree': { one: '{count} worktree', other: '{count} worktrees' },
  'counts.dirty': { one: '{count} com alterações', other: '{count} com alterações' },

  // Cartão de execução (Inv §1.5)
  'run.changed_files': { one: '✎ alterou {count} arquivo', other: '✎ alterou {count} arquivos' },
  'run.changed_files_unknown': '✎ alterou arquivos',
  'run.talk': '💬 só conversa',
  'run.next_step': 'Próximo passo: {step}',
  'run.summary_pending': 'resumo pendente',
  'run.summary_failed': 'resumo indisponível',
  'hidden.empty': 'vazia',
  'hidden.automation': 'automação',
  'hidden.subagent': 'subagente',
  'hidden.internal': 'interna',
  'end.in_progress': 'em andamento',
  'end.exit': '/exit',
  'end.clear': '/clear',
  'end.logout': 'logout',
  'end.idle': 'sem /exit (inativa)',
  'end.clean': 'encerrada',
  'end.killed': 'Ctrl+C/fechada',
  'interval.today': 'hoje',
  'interval.yesterday': 'ontem',

  // Botões
  'action.resume': '▶ retomar',
  'action.resume_tool': '▶ retomar ({tool})',
  'action.open_folder': 'abrir pasta',

  // Projetos (Inv §1.2, §1.3)
  'projects.title': 'Projetos',
  'projects.empty': 'Nenhum projeto registrado ainda.',
  'projects.ongoing_count': { one: '{count} ongoing em andamento', other: '{count} ongoings em andamento' },
  'project.ongoing': '🔄 Em andamento · {count}',
  'project.sessions': 'Sessões',
  'project.show_hidden': 'mostrar sessões ocultas',
  'project.no_sessions': 'Nenhuma sessão registrada neste projeto.',
  'project.clear_chain': 'continuação via /clear',

  // Paginador
  'pager.label': 'Paginação',
  'pager.newer': '← mais recentes',
  'pager.page': 'página {page} de {pages}',
  'pager.older': 'mais antigas →',

  // Sessões (Inv §1.4)
  'sessions.title': 'Sessões',
  'sessions.empty': 'Nenhuma sessão encontrada.',
  'sessions.tool': 'Ferramenta',
  'sessions.project': 'Projeto',
  'sessions.kind': 'Tipo',
  'sessions.all_tools': 'todas as ferramentas',
  'sessions.all_projects': 'todos os projetos',
  'sessions.all_kinds': '✎ e 💬',
  'sessions.changed': '✎ alterou arquivos',
  'sessions.talk': '💬 só conversa',

  // Busca (Inv §1.6)
  'search.label': 'Buscar sessões',
  'search.placeholder': 'Buscar em projetos, branches e resumos…',
  'search.empty': 'Nada encontrado.',
  'search.no_project': 'sem projeto',
};
