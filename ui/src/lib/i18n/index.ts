// Minimal i18n: `t(key, params?)` in the current language, `tr(lang, …)` in a given one.
import { en, type Entry, type Key } from './en';
import { ptBR } from './pt-BR';
import type { Lang } from '../types';

export type { Key } from './en';
export type Params = Record<string, string | number>;

const dictionaries: Record<Lang, Record<Key, Entry>> = { en, 'pt-BR': ptBR };

/** `pt*` → pt-BR, anything else → en. */
export function detectLang(tag: string | undefined | null): Lang {
  return tag && tag.toLowerCase().startsWith('pt') ? 'pt-BR' : 'en';
}

let current: Lang = detectLang(typeof navigator === 'undefined' ? undefined : navigator.language);

export function getLang(): Lang {
  return current;
}

export function setLang(lang: Lang): void {
  current = lang;
}

const pluralRules = new Map<Lang, Intl.PluralRules>();

function pluralCategory(lang: Lang, n: number): Intl.LDMLPluralRule {
  let rules = pluralRules.get(lang);
  if (!rules) {
    rules = new Intl.PluralRules(lang);
    pluralRules.set(lang, rules);
  }
  return rules.select(n);
}

/** The text of `key` in `lang`; plural entries use `params.count`. Unknown params stay as `{name}`. */
export function tr(lang: Lang, key: Key, params?: Params): string {
  const entry = dictionaries[lang][key] ?? en[key] ?? key;
  let text: string;
  if (typeof entry === 'string') {
    text = entry;
  } else {
    const n = Number(params?.count ?? 0);
    text = pluralCategory(lang, n) === 'one' ? entry.one : entry.other;
  }
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (m, name: string) => (name in params ? String(params[name]) : m));
}

export function t(key: Key, params?: Params): string {
  return tr(current, key, params);
}
