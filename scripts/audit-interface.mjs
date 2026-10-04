#!/usr/bin/env node
/**
 * Audit mesuré de l'interface dans le harnais navigateur (`pnpm dev`).
 *
 * Vérifie, écran par écran, à 1 100 px de large :
 *   - aucun débordement horizontal (ni de la page, ni des tableaux internes) ;
 *   - un libellé accessible sur chaque bouton d'icône ;
 *   - un focus visible au clavier ;
 *   - Échap ne ferme jamais les réglages en perdant une saisie.
 *
 * Usage : `pnpm dev` dans un terminal, puis
 *   node scripts/audit-interface.mjs [http://localhost:1420]
 * Playwright n'est pas une dépendance du projet : il est cherché dans le
 * NODE_PATH (installation globale), avec Chromium.
 * Code de sortie 1 au premier écran non conforme : utilisable comme test.
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let chromium;
try {
  ({ chromium } = require("playwright"));
} catch {
  console.error("Playwright introuvable : installe-le globalement (npm i -g playwright) ou ajoute-le au NODE_PATH.");
  process.exit(2);
}

const URL = process.argv[2] ?? "http://localhost:1420";
const failures = [];
const check = (ok, what) => { if (!ok) failures.push(what); console.log(`${ok ? "✓" : "✗"} ${what}`); };

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
await page.goto(URL);
await page.waitForTimeout(1500);

async function audit(screen) {
  const r = await page.evaluate(() => {
    const visible = (el) => { const cs = getComputedStyle(el); const b = el.getBoundingClientRect(); return cs.display !== "none" && cs.visibility !== "hidden" && b.width > 0; };
    const outside = [...document.querySelectorAll("body *")]
      .filter((el) => visible(el) && !el.closest("svg, canvas, .wf-canvas, .term, .xterm"))
      .filter((el) => el.getBoundingClientRect().right > window.innerWidth + 1)
      .map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join(".")}`);
    const scrolled = [...document.querySelectorAll(".routes, .grants-table, .history-head, .settings-body")]
      .filter((el) => visible(el) && el.scrollWidth > el.clientWidth + 1)
      .map((el) => `${el.className} ${el.scrollWidth}/${el.clientWidth}`);
    const unlabeled = [...document.querySelectorAll("button")]
      .filter((b) => visible(b) && !/[A-Za-zÀ-ÿ0-9]/.test(b.textContent ?? "") && !b.getAttribute("aria-label") && !b.getAttribute("title"))
      .map((b) => `"${(b.textContent ?? "").trim()}" (${b.className})`);
    return { page: document.documentElement.scrollWidth - window.innerWidth, outside: outside.slice(0, 5), scrolled, unlabeled };
  });
  check(r.page <= 0 && r.outside.length === 0, `${screen} : aucun élément hors de la fenêtre ${r.outside.join(", ")}`);
  check(r.scrolled.length === 0, `${screen} : aucun défilement horizontal interne ${r.scrolled.join(", ")}`);
  check(r.unlabeled.length === 0, `${screen} : boutons d'icône libellés ${r.unlabeled.join(", ")}`);
}

await audit("monde");

// Focus visible : la tabulation doit dessiner un contour net.
await page.keyboard.press("Tab");
const focus = await page.evaluate(() => {
  const cs = getComputedStyle(document.activeElement);
  return { tag: document.activeElement.tagName, outline: cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) >= 2, shadow: cs.boxShadow !== "none" };
});
check(focus.outline || focus.shadow, `focus visible au clavier (${focus.tag})`);

await page.getByRole("button", { name: /Réglages/ }).click();
await page.waitForTimeout(400);
const sections = await page.locator(".settings-nav-item").allInnerTexts();
for (let i = 0; i < sections.length; i++) {
  await page.locator(".settings-nav-item").nth(i).click();
  await page.waitForTimeout(500);
  await audit(`réglages › ${sections[i].split("\n")[0]}`);
}

// Échap ne perd jamais une saisie : modification non enregistrée, focus hors
// du champ, Échap → l'écran reste ouvert, la saisie aussi.
await page.locator(".settings-nav-item").nth(1).click();
await page.waitForTimeout(400);
const name = page.locator(".split-detail input.input").first();
const before = await name.inputValue();
await name.fill(`${before} (modifié)`);
await page.locator(".split-detail h2").click();
await page.keyboard.press("Escape");
await page.waitForTimeout(200);
check(await page.locator(".settings").isVisible(), "Échap avec une saisie non enregistrée : les réglages restent ouverts");
check((await name.inputValue()) === `${before} (modifié)`, "Échap : la saisie est conservée");
check(await page.locator(".unsaved-guard").isVisible(), "Échap : l'avertissement de modifications non enregistrées est affiché");
await page.getByRole("button", { name: "Quitter sans enregistrer" }).click();
await page.waitForTimeout(200);
check(!(await page.locator(".settings").isVisible()), "« Quitter sans enregistrer » ferme les réglages");

// Sans modification, Échap ferme comme avant.
await page.getByRole("button", { name: /Réglages/ }).click();
await page.waitForTimeout(400);
await page.locator(".settings-nav-item").first().click();
await page.keyboard.press("Escape");
await page.waitForTimeout(200);
check(!(await page.locator(".settings").isVisible()), "Échap sans modification ferme les réglages");

await page.getByRole("button", { name: "Historique" }).click();
await page.waitForTimeout(800);
await audit("historique");
await page.keyboard.press("Escape");
await page.waitForTimeout(200);
check(!(await page.locator(".history").isVisible()), "Échap ferme l'historique");

check(errors.length === 0, `aucune erreur dans la console ${errors.join(" | ")}`);
await browser.close();
if (failures.length) {
  console.error(`\n${failures.length} point(s) non conforme(s).`);
  process.exit(1);
}
console.log("\nInterface conforme.");
