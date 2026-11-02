// Minimal i18n: `t(key, params?)` in the current language, `tr(lang, …)` in a given one.
import { en, type Entry, type Key } from './en';
import { ptBR } from './pt-BR';
import type { Lang } from '../types';
import { langState } from './lang.svelte';

export type { Key } from './en';
export type Params = Record<string, string | number>;

const dictionaries: Record<Lang, Record<Key, Entry>> = { en, 'pt-BR': ptBR };

/** `pt*` → pt-BR, anything else → en. */
export function detectLang(tag: string | undefined | null): Lang {
  return tag && tag.toLowerCase().startsWith('pt') ? 'pt-BR' : 'en';
}

const systemLang: Lang = detectLang(typeof navigator === 'undefined' ? undefined : navigator.language);

export function getLang(): Lang {
  return langState.lang ?? systemLang;
}

export function setLang(lang: Lang): void {
  langState.lang = lang;
}

/** The text of `key` in `lang`; plural entries use `params.count`. Unknown params stay as `{name}`. */
export function tr(lang: Lang, key: Key, params?: Params): string {
  const entry = dictionaries[lang][key] ?? en[key] ?? key;
  let text: string;
  if (typeof entry === 'string') {
    text = entry;
  } else {
    const n = Number(params?.count ?? 0);
    // The v1 rule (`count === 1 ? singular : plural`) in both languages: CLDR would put 0 in
    // "one" for pt, but the v1 says "0 interrompidas".
    text = n === 1 ? entry.one : entry.other;
  }
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (m, name: string) => (name in params ? String(params[name]) : m));
}

export function t(key: Key, params?: Params): string {
  return tr(getLang(), key, params);
}
