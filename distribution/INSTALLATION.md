# Installer Souffle — quel fichier faut-il ouvrir

Ce dossier contient tout ce qu'il faut. **Un seul fichier vous concerne**, celui qui correspond à
votre ordinateur.

```
Souffle/
├── 1 — À quoi sert Souffle.md        ← ce que fait l'application
├── 2 — Installation.md               ← le fichier que vous lisez
├── macOS/
│   ├── Souffle — Mac Apple Silicon (M1 M2 M3 M4).dmg
│   └── Souffle — Mac Intel.dmg
└── Windows/
    └── Souffle — Windows.exe
```

---

## Sur un Mac

### Lequel des deux fichiers ?

Menu  → **À propos de ce Mac**, et regardez la ligne « Puce » ou « Processeur » :

| Ce qui est écrit | Le fichier à ouvrir |
|---|---|
| Puce **Apple M1**, M2, M3, M4… | `Souffle — Mac Apple Silicon (M1 M2 M3 M4).dmg` |
| Processeur **Intel** | `Souffle — Mac Intel.dmg` |

Dans le doute, prenez la version Apple Silicon : tous les Mac vendus depuis fin 2020 en sont
équipés.

### Les étapes

1. **Double-cliquez sur le fichier `.dmg`.** Une fenêtre s'ouvre avec l'icône de Souffle.
2. **Glissez l'icône Souffle sur le dossier Applications**, dans la même fenêtre.
3. Ouvrez le dossier **Applications** et, cette première fois seulement, **faites un clic droit sur
   Souffle → Ouvrir**, puis confirmez **Ouvrir** dans la boîte de dialogue.

> **Pourquoi ce clic droit ?** L'application n'est pas signée par un certificat Apple payant. Un
> simple double-clic affiche « Souffle ne peut pas être ouvert car son développeur n'a pas pu être
> vérifié ». Le clic droit → Ouvrir contourne cet avertissement une fois pour toutes.
>
> Si macOS refuse quand même : **Réglages Système → Confidentialité et sécurité**, descendez jusqu'à
> la section Sécurité, et cliquez sur **Ouvrir quand même** à côté du nom de Souffle.

### Deux autorisations à accorder

Souffle les demande au bon moment, mais autant savoir à quoi elles servent :

- **Micro** — demandé automatiquement à la première dictée. Sans lui, rien ne peut fonctionner.
- **Accessibilité** — **Réglages Système → Confidentialité et sécurité → Accessibilité**, puis
  cochez Souffle. C'est ce qui permet à l'application d'envoyer le `⌘V` qui colle votre texte dans
  l'application active. Sans cette autorisation, le texte est copié dans le presse-papier mais pas
  collé — vous devez le coller vous-même. Le bouton dans les réglages de Souffle ouvre directement
  le bon volet.

### Le raccourci

**`⌥ Espace`** (Option + Espace). Appuyez, parlez, ré-appuyez.

---

## Sur un PC Windows

### Le fichier à ouvrir

`Souffle — Windows.exe` — il n'y en a qu'un, valable pour tous les PC 64 bits.

### Les étapes

1. **Double-cliquez sur `Souffle — Windows.exe`.**
2. Un écran bleu apparaît : **« Windows a protégé votre ordinateur »**. C'est normal — l'application
   n'est pas signée par un certificat commercial, et Windows le signale pour tout programme peu
   diffusé.
   Cliquez sur **Informations complémentaires**, puis sur **Exécuter quand même**.
3. L'installation se fait toute seule, sans rien demander. Souffle démarre à la fin.

Aucune autorisation à configurer sous Windows : le collage fonctionne directement.

### Le raccourci

**`Ctrl + Maj + Espace`**. Appuyez, parlez, ré-appuyez.

> Ce n'est pas `Ctrl + Espace` : Windows réserve cette combinaison au changement de langue du
> clavier, elle n'était donc pas utilisable.

---

## Au premier lancement, des deux côtés

La fenêtre de réglages s'ouvre directement sur l'onglet **Intelligence**. Il y a **une seule chose à
faire** : coller une clé API (OpenAI ou Groq), puis fermer la fenêtre.

Sans cette clé, Souffle enregistre votre voix mais ne peut rien en faire — il n'a pas de moteur de
transcription à lui.

Ensuite, Souffle vit discrètement :

- **macOS** — dans la barre de menus, en haut à droite, près de l'horloge. Pas d'icône dans le dock.
- **Windows** — dans la zone de notification, en bas à droite. Cliquez sur la flèche `^` si l'icône
  est masquée.

C'est par cette icône qu'on rouvre les réglages, qu'on change la langue parlée, et qu'on quitte
l'application.

---

## Si quelque chose ne va pas

| Ce qui se passe | Ce que ça veut dire |
|---|---|
| **« Micro pris par une autre app »** | Une autre application tient le micro (ChatGPT, Teams, Zoom, un assistant vocal). Quittez-la, ou attendez : Souffle essaie automatiquement les autres entrées audio disponibles. |
| **« Micro refusé par le système »** | L'autorisation micro n'a pas été accordée. Réglages Système → Confidentialité et sécurité → Microphone. |
| **« Copié — autorisez l'Accessibilité »** | macOS uniquement : le texte est dans le presse-papier, l'autorisation d'Accessibilité manque. Voir plus haut. |
| **Le raccourci ne fait rien** | Une autre application l'utilise déjà. Changez-le dans Réglages → Général — le champ enregistre la combinaison que vous tapez. |
| **Le texte n'est pas collé** | Il est resté dans le presse-papier : `⌘V` (ou `Ctrl+V`) le récupère. Rien n'est perdu. |

---

## Désinstaller

- **macOS** — glissez Souffle depuis Applications vers la corbeille. Vos réglages restent dans
  `~/Bibliothèque/Application Support/Souffle` ; supprimez ce dossier pour tout effacer.
- **Windows** — Paramètres → Applications → Souffle → Désinstaller.
