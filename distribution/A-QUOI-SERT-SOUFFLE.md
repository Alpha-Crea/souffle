# Souffle — à quoi ça sert

**Vous appuyez sur un raccourci, vous parlez, et le texte propre s'écrit tout seul là où vous étiez
en train de taper.** Dans un mail, dans Slack, dans Notion, dans un champ de recherche, dans un
terminal. N'importe où.

---

## Le problème que ça règle

On parle environ trois fois plus vite qu'on ne tape. Pourtant presque personne ne dicte, et la
raison est simple : **la dictée classique écrit du brut**. Elle transcrit les « euh », les « enfin
non je voulais dire », les reprises en cours de phrase, sans ponctuation ni majuscules. Résultat, on
passe à corriger le temps qu'on a gagné à parler. Alors on retourne au clavier.

Souffle ajoute l'étape qui manquait. Deux modèles travaillent à la chaîne :

1. **le premier transcrit** ce que vous avez dit ;
2. **le second réécrit** : il enlève les hésitations, ponctue, met les majuscules et les accents,
   ne garde que la version finale quand vous vous êtes repris, et adapte le ton à l'application où
   le texte va atterrir.

Ce qui sort est du texte que vous pouvez envoyer tel quel.

## Ce que ça change concrètement

- **Les mails et les messages longs.** Un mail de quinze lignes se dicte en trente secondes.
- **Quand on pense mieux en parlant qu'en écrivant.** Comptes rendus, notes de réunion, premiers
  jets, réponses clients, cahiers des charges : dire les choses puis les relire est souvent plus
  rapide que de chercher ses phrases au clavier.
- **Quand les mains ne suivent pas.** Poignets douloureux, tendinite, journée déjà passée à taper.
- **Dans une langue qu'on parle mieux qu'on ne l'écrit.** L'orthographe et la grammaire sont
  corrigées à la sortie.
- **Sans changer d'application.** Pas de fenêtre à ouvrir, pas de copier-coller : le texte arrive
  dans le champ où était déjà votre curseur.

## Ce que ça ne fait pas

Autant le dire tout de suite, ça évite les mauvaises surprises.

- **Ce n'est pas un assistant.** Si vous dictez « quelle heure est-il ? », Souffle écrit la question.
  Il n'y répond pas. Il réécrit votre parole, il ne dialogue jamais avec vous.
- **Pas de mot à mot.** Le texte s'écrit **phrase par phrase**, pas mot après mot. C'est une limite
  assumée : nettoyer « euh, enfin, je voulais dire » suppose d'avoir la phrase entière, et un texte
  déjà collé dans une autre application ne peut plus être repris.
- **Une seule langue par dictée.** Le modèle de transcription choisit une langue par enregistrement.
  Enchaîner français et anglais dans la même phrase le fait dérailler. Relâchez, reprenez.
- **Pas de traduction, sauf si vous la demandez.** Par défaut, vous parlez anglais, le texte sort en
  anglais. La traduction est un réglage explicite.

## Où vont vos paroles

C'est la question qui compte, alors la réponse est directe.

L'audio part chez **le fournisseur que vous avez choisi** — OpenAI ou Groq — pour être transcrit,
puis le texte y repart pour être réécrit. Rien d'autre ne sort de la machine.

Trois choses à savoir :

- **Vos clés API sont chiffrées** par le trousseau du système (Keychain sur macOS, DPAPI sur
  Windows). Elles ne sont jamais écrites en clair sur le disque.
- **L'historique des dictées est local**, et le mode confidentialité le désactive complètement.
- **Le mode 100 % local existe.** Souffle accepte n'importe quel serveur compatible avec l'API
  d'OpenAI : whisper.cpp, LM Studio, Ollama sur votre propre machine. Dans ce cas, **rien ne sort
  de l'ordinateur**, pas même l'audio.

Souffle connaît le **nom** de l'application active — pour adapter le ton entre un message Slack et
un mail — mais jamais son contenu. Il ne lit pas votre écran.

## Ce qu'il faut pour démarrer

Une **clé API** chez OpenAI ou chez Groq. C'est la seule chose à faire une fois, au premier
lancement : la fenêtre de réglages s'ouvre directement au bon endroit, vous collez la clé, vous
fermez.

| Fournisseur | Où | Ce que ça vaut |
|---|---|---|
| **Groq** | console.groq.com | le plus rapide — environ une seconde entre la fin de la phrase et le texte |
| **OpenAI** | platform.openai.com | le plus précis en français |
| **Serveur local** | chez vous | rien ne sort de la machine, au prix d'un peu de configuration |

La facturation se fait à l'usage, directement chez le fournisseur choisi : consultez leur page de
tarifs pour l'ordre de grandeur, il varie selon le modèle retenu. Souffle lui-même ne coûte rien et
ne prend rien au passage.

## Comment on s'en sert, au quotidien

1. Vous placez votre curseur là où le texte doit aller.
2. Vous appuyez sur le raccourci — une petite pilule apparaît en bas de l'écran.
3. Vous parlez.
4. Vous ré-appuyez sur le raccourci (ou vous appuyez sur `Échap`, ou vous cliquez sur la pilule).
5. Une seconde ou deux plus tard, le texte propre s'écrit à l'endroit voulu.

Souffle vit dans la barre de menus (macOS) ou la zone de notification (Windows). Il ne prend pas de
place dans le dock, il n'ouvre aucune fenêtre, et il ne vous vole jamais le focus.

**Si le collage rate** — vous avez changé de fenêtre entre-temps, l'application n'a pas de champ de
saisie — le texte reste dans le presse-papier : un simple `⌘V` (ou `Ctrl+V`) le récupère. Rien n'est
jamais perdu.

## Quelques réglages qui valent le détour

- **Mode direct** — le texte s'écrit phrase par phrase pendant que vous parlez, au lieu d'arriver
  d'un bloc à la fin. Plus vivant, mais légèrement moins précis : chaque phrase est transcrite avec
  moins de contexte que le texte entier.
- **Dictionnaire** — les noms propres, le jargon maison, les noms de produits. « KACHÉ » ne
  deviendra plus « caché ».
- **Raccourcis de texte** — vous dites « mon email », il écrit votre adresse.
- **Ton par application** — détendu dans Slack, soutenu dans Mail, brut dans un terminal.
- **Maintenir pour parler** — au lieu d'appuyer deux fois, vous gardez la touche enfoncée pendant
  que vous parlez.

---

*Souffle est développé par Alpha Agency. Le mode d'emploi complet se trouve dans le fichier
`LISEZ-MOI.html` livré avec l'application.*
