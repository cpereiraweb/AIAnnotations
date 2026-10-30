import { afterEach, describe, expect, it } from 'vitest';
import { en } from './en';
import { ptBR } from './pt-BR';
import { detectLang, getLang, setLang, t, tr } from './index';

function shape(dict: Record<string, unknown>): string[] {
  return Object.entries(dict)
    .map(([k, v]) => (typeof v === 'string' ? k : `${k}{${Object.keys(v as object).sort().join(',')}}`))
    .sort();
}

describe('i18n', () => {
  afterEach(() => setLang('en'));

  it('has the same keys (and plural forms) in en and pt-BR', () => {
    expect(shape(ptBR)).toEqual(shape(en));
  });

  it('has no empty text', () => {
    for (const dict of [en, ptBR]) {
      for (const [key, value] of Object.entries(dict)) {
        const texts = typeof value === 'string' ? [value] : Object.values(value);
        for (const text of texts) expect(text, key).not.toBe('');
      }
    }
  });

  it('detects the language from navigator.language', () => {
    expect(detectLang('pt-BR')).toBe('pt-BR');
    expect(detectLang('pt')).toBe('pt-BR');
    expect(detectLang('pt-PT')).toBe('pt-BR');
    expect(detectLang('en-US')).toBe('en');
    expect(detectLang('de')).toBe('en');
    expect(detectLang(undefined)).toBe('en');
  });

  it('interpolates params and picks plurals with Intl.PluralRules', () => {
    expect(tr('pt-BR', 'inbox.title', { count: 3 })).toBe('Precisa de você · 3');
    expect(tr('pt-BR', 'run.changed_files', { count: 1 })).toBe('✎ alterou 1 arquivo');
    expect(tr('pt-BR', 'run.changed_files', { count: 4 })).toBe('✎ alterou 4 arquivos');
    expect(tr('en', 'run.changed_files', { count: 1 })).toBe('✎ changed 1 file');
    expect(tr('en', 'run.changed_files', { count: 4 })).toBe('✎ changed 4 files');
    expect(tr('pt-BR', 'badge.stale_branch', { count: 1 })).toBe('sem merge há 1 dia');
    expect(tr('pt-BR', 'badge.stale_branch', { count: 5 })).toBe('sem merge há 5 dias');
  });

  it('uses the v1 plural rule (only 1 is singular), so 0 is plural in pt-BR too', () => {
    expect(tr('pt-BR', 'run.changed_files', { count: 0 })).toBe('✎ alterou 0 arquivos');
    expect(tr('en', 'run.changed_files', { count: 0 })).toBe('✎ changed 0 files');
    expect(tr('pt-BR', 'counts.interrupted', { count: 0 })).toBe('0 interrompidas');
    expect(tr('pt-BR', 'badge.stale_branch', { count: 0 })).toBe('sem merge há 0 dias');
  });

  it('t uses the current language', () => {
    setLang('pt-BR');
    expect(getLang()).toBe('pt-BR');
    expect(t('inbox.empty')).toBe('Nada pendente. Bom trabalho.');
    expect(t('toast.briefing_running')).toBe('O briefing já está sendo gerado.');
    setLang('en');
    expect(t('inbox.empty')).toBe('Nothing pending. Good job.');
  });
});
