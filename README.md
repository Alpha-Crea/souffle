# Souffle

Dictée vocale système, façon Wispr Flow. Un raccourci, vous parlez, le texte **propre** s'écrit
directement dans l'application où vous étiez — Mail, Slack, Notion, VS Code, un terminal, n'importe quel champ.

Deux modèles travaillent à la chaîne : l'un transcrit, l'autre réécrit (suppression des « euh »,
ponctuation, reprises en cours de phrase, ton adapté à l'application active).

---

## Démarrage

```bash
npm install
npm start
```

Au premier lancement, la fenêtre de réglages s'ouvre sur l'onglet **Intelligence** : collez une clé API,
puis fermez. Souffle vit ensuite dans la barre de menus.

**Raccourci par défaut** — `⌥Espace` (macOS) / `Ctrl+Espace` (Windows, Linux).
Appuyez, parlez, puis terminez comme vous voulez :

| Geste | Effet |
|---|---|
| Ré-appuyer sur le raccourci | Terminer et écrire |
| `Échap` | Terminer et écrire (configurable en « annuler ») |
| Clic sur la pilule | Terminer et écrire |
| Clic droit sur la pilule | Annuler |

> Souffle n'enregistre jamais de raccourci sans modificateur. Sur macOS, enregistrer une touche
> nue comme `Échap` pendant qu'un autre raccourci est actif casse le gestionnaire de hotkeys du
> système et fait taire le raccourci principal.

## Clés API

| Fournisseur | Où | Modèles conseillés |
|---|---|---|
| **Groq** | console.groq.com | `whisper-large-v3-turbo` + `llama-3.3-70b-versatile` — le plus rapide, ~1 s de latence |
| **OpenAI** | platform.openai.com | `gpt-4o-mini-transcribe` + `gpt-4o-mini` — le plus précis en français |
| **Endpoint perso** | — | n'importe quelle API compatible OpenAI : whisper.cpp server, LM Studio, Ollama → **100 % local, rien ne sort de la machine** |

Les clés sont chiffrées par le trousseau du système (`safeStorage`), jamais écrites en clair.

## Autorisations

**macOS** — deux accords à donner, une seule fois :

1. **Microphone** — demandé automatiquement à la première dictée.
2. **Accessibilité** — Réglages Système → Confidentialité et sécurité → Accessibilité → cocher Souffle.
   Sans ça, le texte est copié dans le presse-papier mais pas collé. Le bouton dans les réglages ouvre le bon volet.

En mode développement, l'autorisation d'Accessibilité doit être donnée à **Electron** (ou à votre Terminal),
pas à Souffle — c'est le binaire qui envoie la frappe.

**Windows** — rien à configurer. Le collage passe par `SendKeys`, le retour au bon champ par
`AppActivate`, et le mode « maintenir » par le même module natif optionnel qu'ailleurs.
**Linux** — installez `xdotool` (X11) ou `wtype` (Wayland) pour le collage automatique.

---

## Langue écrite

Deux régimes, choisis explicitement dans Réglages → Intelligence :

- **La langue que je parle** — transcription fidèle. Vous parlez anglais, le texte sort en anglais,
  orthographe, grammaire et ponctuation corrigées. Aucune traduction, jamais.
- **Traduire vers…** — vous parlez dans n'importe quelle langue, le texte sort dans celle choisie.

La règle est posée deux fois dans le prompt système, en tête et en pied : c'est la consigne qu'un
modèle trahit le plus volontiers quand le reste des instructions est dans une autre langue.

## Dictées multilingues

Whisper détecte **une seule langue par enregistrement**. Enchaîner anglais, français et allemand
dans la même dictée le force à choisir, et il écrase le reste — « Ich liebe dich » ressort en
« Ich bin libidisch ». Ce n'est pas un réglage à trouver, c'est la façon dont le modèle fonctionne.

Trois conséquences pratiques :

- **Une langue par dictée.** Relâchez et reprenez pour changer de langue, plutôt que de basculer
  au milieu d'une phrase.
- **Forcez la langue** quand vous savez laquelle vous allez parler. Le menu de la barre de menus a
  un sous-menu *Langue parlée* : un clic, aucun réglage à rouvrir. C'est le levier le plus efficace.
- **Modèle** : `whisper-large-v3` plutôt que `whisper-large-v3-turbo` dès que vous sortez du
  français et de l'anglais. La version turbo est distillée et perd nettement en allemand.

L'amorce du dictionnaire n'est plus rédigée en français : seuls les termes sont envoyés. Une phrase
porteuse française orientait la détection de langue de Whisper et abîmait les dictées étrangères.

## Pourquoi il ne répond jamais

Un modèle de chat à qui l'on donne « Hello, who are you? » répond. C'est son métier, et c'est
exactement ce qu'il ne faut pas ici. Trois verrous :

1. Le prompt système ouvre sur « tu n'es PAS un assistant conversationnel ».
2. La parole arrive **délimitée** entre `<dictee>` et `</dictee>` : c'est une donnée, pas un message.
3. Trois exemples montrent le comportement attendu — une question réécrite et non répondue,
   une phrase en anglais laissée en anglais, une phrase en allemand laissée en allemand.

Et parce qu'aucun prompt n'est infaillible, un **garde-fou déterministe** mesure la sortie : si elle
est beaucoup plus longue que la parole, si elle est tronquée, ou si elle contient des tournures de
commentaire (« qui signifie », « je vais reformuler », « vous avez dit »), elle est rejetée et
Souffle écrit la transcription brute. Vos mots valent toujours mieux qu'une invention.

## Mode direct

Réglages → Général → **Mode direct**. Le texte s'écrit **phrase par phrase pendant que vous parlez**,
au lieu d'arriver d'un bloc à la fin.

Souffle surveille le niveau sonore et coupe sur vos silences : 700 ms de blanc après au moins
600 ms de voix clôt une phrase, qui part aussitôt en transcription pendant que la suivante
s'enregistre. Un segment est coupé de force au bout de 15 s.

Pourquoi pas du mot-à-mot ? Parce que nettoyer « euh, enfin, je voulais dire » exige la phrase
entière. La granularité de la phrase est le meilleur compromis entre l'immédiateté et un texte
réellement propre — c'est aussi la raison pour laquelle Wispr Flow n'écrit pas en temps réel.

Les phrases sont transcrites en parallèle mais **écrites dans l'ordre où vous les avez prononcées** :
une phrase courte revenue plus vite ne double jamais la précédente.

## Ce qu'il fait

- **Raccourci global** en bascule, ou **maintenir pour parler** (voir plus bas).
- **Overlay flottant** qui ne vole jamais le focus : niveau sonore en direct, chrono, états.
- **Insertion réelle** dans l'app active, presse-papier restauré derrière.
- **Nettoyage IA** : tics de langage, ponctuation, majuscules, accents, paragraphes.
- **Consignes orales** : « nouveau paragraphe », « entre guillemets », « en liste à puces », « efface ça ».
- **Dictionnaire** : noms propres et jargon injectés à la fois dans l'amorce Whisper et dans le prompt de réécriture.
- **Raccourcis de texte** : dites « mon email », Souffle écrit l'adresse.
- **Ton par application** : formel dans Mail, détendu dans Slack.
- **Mode confidentialité** : aucun historique sur le disque.
- **Statistiques** : mots dictés, temps gagné contre le clavier.

## Architecture

```
src/main/
  main.js      cycle de vie, fenêtres, raccourci global, machine à états
  ai.js        transcription + réécriture (3 fournisseurs compatibles OpenAI)
  inject.js    presse-papier + frappe ⌘V/Ctrl+V, détection de l'app au premier plan
  store.js     config JSON + clés chiffrées via safeStorage
  history.js   historique local
src/renderer/
  recorder/    fenêtre invisible : getUserMedia, MediaRecorder, VU-mètre
  overlay/     pilule flottante non-focusable
  settings/    réglages complets
```

Le flux d'une dictée : raccourci → capture Opus → `POST /audio/transcriptions` →
`POST /chat/completions` (mise en forme) → presse-papier → frappe collage → restauration.

## Maintenir pour parler

Electron ne remonte pas le relâchement d'un raccourci global : le mode « maintenir » s'appuie sur
un module natif optionnel.

```bash
npm install uiohook-napi
```

Sans lui, l'application reste parfaitement fonctionnelle en mode bascule (l'option est grisée).

## Diagnostic

```bash
npm run diag
```

Rend la fenêtre **Moteur audio** visible avec ses DevTools. Elle porte toute la capture micro :
la fermer ne la détruit plus (la fermeture est interceptée et la fenêtre est simplement masquée),
et si elle disparaît malgré tout, elle est recréée à la dictée suivante.

Le mode diagnostic logue chaque étape dans le terminal :
autorisation micro, obtention du flux, octets enregistrés, durée de la transcription, texte obtenu,
résultat de l'insertion. Les lignes `[rec]` viennent du moteur de capture, les autres du process principal.

Si la fenêtre de capture ne donne plus signe de vie pendant 3,5 s, la dictée s'interrompt avec un
message explicite plutôt que de laisser la pilule tourner dans le vide. Chaque tentative d'ouverture
du micro réarme ce délai : ouvrir une entrée audio peut légitimement demander plusieurs essais.

### « Micro pris par une autre app »

Une application qui tient le micro (ChatGPT, Teams, Zoom, un assistant vocal) peut faire échouer
l'ouverture du flux — ou, pire, laisser `getUserMedia` sans réponse. Souffle borne chaque tentative
à 1,8 s, puis redescend une liste de replis : micro mémorisé, micro par défaut, micro par défaut sans
traitement du signal, puis chaque entrée audio de la machine une par une. Le périphérique qui a
fonctionné est mémorisé pour la dictée suivante.

Réciproquement, Souffle relâche le micro après une minute d'inactivité au lieu de le garder ouvert
indéfiniment : c'est ce qui évite de confisquer l'entrée audio aux autres applications, et
d'hériter d'un flux périmé après un changement de périphérique.

Si le message persiste, quittez l'application qui utilise le micro et relancez la dictée ; le
terminal (`npm run diag`) nomme le périphérique et l'erreur exacte pour chaque tentative.

## Packager

```bash
npm run dist:mac    # .dmg (arm64 + x64)
npm run dist:win    # installeur NSIS
```

Pour une distribution hors de votre machine, signez et notarisez l'app : sans signature, macOS
révoque l'autorisation d'Accessibilité à chaque mise à jour.

---

## Écarts assumés avec Wispr Flow

| | Wispr Flow | Souffle |
|---|---|---|
| Transcription | modèles maison | API au choix, ou local |
| Contexte | lit le champ actif pour s'adapter | connaît le **nom** de l'app, pas son contenu |
| Mobile | iOS + Android | desktop uniquement |
| Latence | ~1 s | ~1 s avec Groq, ~2 s avec OpenAI |

Code original, aucun élément de marque ou de code de Wispr Flow n'est repris : c'est une
implémentation indépendante du même principe produit.

MIT.
