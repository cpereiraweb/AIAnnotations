import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { getLang } from './lib/i18n';

document.documentElement.lang = getLang();

const app = mount(App, { target: document.getElementById('app')! });

export default app;
