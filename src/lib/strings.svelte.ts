// All user-facing text, in the chosen language (the texts live in i18n/). Everything here reads the
// current language when it's used, so components update at once when the language changes.
import type { CatalogApp, Category, Target, TaskStatus, TweakCategory, WslAction } from "./types";
import { en, type Strings } from "./i18n/en";
import { ptBR } from "./i18n/pt-BR";

export type Language = "en" | "pt-BR";

const locales: Record<Language, Strings> = { en, "pt-BR": ptBR };
const locale = $state<{ language: Language }>({ language: "en" });

/** Languages for the Settings picker, each named in itself. */
export const languages: { value: Language; label: string }[] = [
  { value: "en", label: en.languageName },
  { value: "pt-BR", label: ptBR.languageName },
];

export function setLanguage(language: Language): void {
  locale.language = language;
  document.documentElement.lang = language;
}

export function systemLanguage(): Language {
  return navigator.language.toLowerCase().startsWith("pt") ? "pt-BR" : "en";
}

function current(): Strings {
  return locales[locale.language];
}

/** One section of the current language's strings, looked up on every read. */
function section<K extends keyof Strings>(key: K): Strings[K] {
  return new Proxy({}, { get: (_target, prop) => Reflect.get(current()[key] as object, prop) }) as Strings[K];
}

export const text: Strings = new Proxy({} as Strings, {
  get: (_target, prop) => Reflect.get(current(), prop),
});

export const categoryOrder: Category[] = [
  "browsers",
  "dev",
  "communication",
  "media",
  "utilities",
  "gaming",
  "productivity",
  "security",
];

export const categoryLabels: Record<Category, string> = section("categories");

export const tweakCategoryOrder: TweakCategory[] = ["appearance", "explorer", "taskbar", "privacy", "performance"];

export const tweakCategoryLabels: Record<TweakCategory, string> = section("tweakCategories");

export const taskStatusLabels: Record<TaskStatus | "pending" | "running", string> = section("taskStatus");

/** Where the app comes from, e.g. "winget", "Microsoft Store", "GitHub", "Flathub", "apt in WSL". */
export function sourceLabel(app: CatalogApp, target: Target = "host"): string | null {
  const sources = current().sources;
  if (target === "wsl") return app.wslResolution?.status === "available" ? sources.aptInWsl : null;
  if (app.resolution.status !== "available") return null;
  switch (app.resolution.manager) {
    case "apt":
    case "dnf":
    case "pacman":
      return app.resolution.manager;
    case "github":
      return sources.github;
    case "flatpak": {
      const flatpak = app.packages.flatpak;
      return typeof flatpak === "object" ? `Flatpak (${flatpak.remote.name})` : sources.flathub;
    }
    case "winget": {
      const winget = app.packages.winget;
      return typeof winget === "object" && winget.source === "msstore" ? sources.msstore : "winget";
    }
  }
}

export function wslActionLabel(action: WslAction): string {
  const labels = current().wslActions;
  switch (action.kind) {
    case "installDistro":
      return labels.installDistro(action.distro);
    default:
      return labels[action.kind];
  }
}
