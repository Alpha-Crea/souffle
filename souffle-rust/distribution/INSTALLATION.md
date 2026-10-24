# Installer Souffle — version native

**Un seul fichier vous concerne**, celui qui correspond à votre ordinateur.

| Votre ordinateur | Le fichier à ouvrir |
|---|---|
| **Mac** (Apple Silicon ou Intel) | double-cliquez sur **`Installer sur ce Mac.command`** |
| **PC Windows** (10 ou 11, 64 bits) | ouvrez le dossier **`Windows`**, puis double-cliquez sur **`Souffle-Installateur-0.3.9.exe`** |

Vos réglages et votre historique sont repris automatiquement depuis l'ancienne version.
Seule la **clé API** est à recoller une fois (onglet *Intelligence*) : l'ancienne application la
gardait chiffrée d'une façon que la nouvelle ne peut pas relire.

---

## Ce que contient ce dossier

```
Souffle/
├── 1 — À quoi sert Souffle.md     ← ce que fait l'application
├── 2 — Installation.md            ← le fichier que vous lisez
├── Installer sur ce Mac.command   ← Mac : à double-cliquer
├── Windows/
│   └── Souffle-Installateur-0.3.9.exe   ← PC : à double-cliquer
└── application/                   ← le code de Souffle (utilisé par l'installation Mac)
```

---

## Sur un Mac

1. **Double-cliquez** sur `Installer sur ce Mac.command`.
   Si macOS répond « Apple n'a pas pu confirmer… » : cliquez sur **Terminé** (surtout pas
   « Placer dans la corbeille »), puis au choix :
   - **Réglages Système → Confidentialité et sécurité**, tout en bas : **Ouvrir quand même**,
     puis double-cliquez de nouveau sur le fichier et confirmez ;
   - ou ouvrez le **Terminal**, tapez `bash ` (avec l'espace), **glissez le fichier** dans la
     fenêtre du Terminal, puis appuyez sur **Entrée**.
2. Une fenêtre de Terminal s'ouvre et fait tout. La **première fois**, elle installe les outils
   nécessaires pour construire Souffle sur votre Mac :
   - les **outils de développement d'Apple** : une fenêtre d'Apple s'ouvre, cliquez sur
     **Installer** ;
   - **Rust**, le langage dans lequel Souffle est écrit (automatique).

   Comptez **10 à 20 minutes** la première fois, deux ou trois minutes les fois suivantes.
3. L'ancienne version est fermée et remplacée. Souffle s'ouvre tout seul.
4. Une seule fois :
   - collez votre **clé API** dans les réglages (*Intelligence*) ;
   - autorisez l'**Accessibilité** quand macOS le demande (Réglages Système → Confidentialité et
     sécurité → Accessibilité → Souffle). Sans elle, le texte est copié mais pas collé ;
   - le **micro** est demandé à la première dictée.

> **Pourquoi construire sur le Mac ?** Une application Mac distribuée hors de l'App Store doit
> être signée par un certificat Apple payant ; sans lui, macOS la bloque. Construite sur votre
> propre Mac, elle n'a pas ce problème.

## Sur un PC Windows

1. Ouvrez le dossier **`Windows`** et double-cliquez sur **`Souffle-Installateur-0.3.9.exe`**.
2. Windows affiche « Windows a protégé votre ordinateur » : cliquez sur **Informations
   complémentaires**, puis **Exécuter quand même**. (L'application n'est pas signée par un
   certificat commercial.)
3. L'installateur ferme et désinstalle l'ancienne version Electron, puis installe la nouvelle.
   Rien d'autre à installer : pas de Node.js, pas d'outils.
   **Mises à jour suivantes** : un double-clic sur le nouvel installateur suffit. Il met Souffle
   à jour par-dessus, sans rien demander ni désinstaller, puis le relance. Réglages,
   historique et clé API sont conservés.
4. Collez votre clé API dans les réglages qui s'ouvrent (*Intelligence*).

Souffle vit dans la zone de notification, en bas à droite (flèche **^** si l'icône est masquée).
Un clic gauche ouvre les réglages, un clic droit le menu.

---

## Au quotidien

| Geste | Effet |
|---|---|
| Raccourci (**⌥ Espace** sur Mac, **Alt + Espace** sur PC, modifiable dans les réglages) | démarrer, puis terminer et écrire |
| **Clic** sur la bulle | pause, puis reprise — ce qui est déjà dit part dans l'historique |
| **Clic droit** sur la bulle | petit menu : terminer, pause ou annuler |
| **Glisser** la bulle | la déplacer ; elle reste où vous la posez |
| **Échap** | terminer (réglable : annuler, ou rien) |

## Quoi de neuf dans cette version

- **Beaucoup plus légère** : l'application pèse quelques mégaoctets au lieu de 250, utilise le
  moteur web du système et capte le micro directement, sans navigateur caché. Le verre de la
  bulle est dessiné par macOS lui-même.
- **La bulle en verre** avec son halo arc-en-ciel qui suit la voix, déplaçable, sans ombre, qui
  vous suit sur tous les bureaux et par-dessus le plein écran.
- **Thème clair, sombre ou automatique**, au choix, pour la bulle et les réglages.
- **Rien ne se perd** : l'audio s'écrit sur le disque pendant que vous parlez. Même un
  plantage en pleine phrase ne fait perdre que la dernière seconde ; la dictée est retranscrite
  au redémarrage.
- **Bascule automatique Groq ↔ OpenAI** quand l'un est bloqué (VPN, panne, accès refusé), si
  les deux clés sont renseignées.
- **Dictées longues** : jusqu'à 20 minutes. Au-delà de quatre minutes, la dictée est découpée
  sur un silence et transcrite pendant que vous parlez encore : la fin arrive aussi vite.
- **Meilleure reformulation chez Groq** (`openai/gpt-oss-120b`) : elle comprend mieux vos
  reprises (« non, jeudi », « oublie ça », « retiens juste… »). Si ce modèle venait à être
  refusé, Souffle revient tout seul à l'ancien.

## Revenir à l'ancienne version

Si quelque chose ne va pas, l'archive précédente (version 0.2.0, Electron) se réinstalle comme
avant ; vos réglages et votre historique n'ont pas été touchés. Le journal de la nouvelle
version (`souffle.log`) se trouve dans *Réglages → Confidentialité → Ouvrir le dossier de Souffle*.
