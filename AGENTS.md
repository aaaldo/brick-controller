# AGENTS.md — Brick as Controller

## Projet

Brick as Controller transforme une **TrimUI Brick sous NextUI** en manette
Bluetooth HID pour PC, Mac, Linux/SteamOS, Android et iPadOS. Le projet est un
**pak NextUI** : aucune modification du cœur de NextUI n'est nécessaire.

Base technique : **Padpod** (Rust, GPL-3) pour le rôle HID Bluetooth.

Périmètre actuel :

- **Phase 1** : `tg5040` — Brick standard uniquement.
- **Phase 2** : autres modèles `tg5040` (Brick Pro, Smart Pro).
- **Phase 3** : `tg5050` (Smart Pro S).
- **H700** : plus tard, hors périmètre immédiat.
- **Hôtes** : macOS, Windows, Linux/SteamOS, Android, iPadOS.
- **Profils HID** : PC (HID générique) et Xbox (identité Xbox Wireless
  Controller), choisis **par hôte** depuis les réglages.
- **Navigation de l'app** : F1 (réglages), F2 (moniteur d'entrées), F1+F2
  (quitter). Start et Select restent des boutons de la manette envoyés à
  l'hôte.
- **USB HID** : hors périmètre du MVP. Le noyau stock de la Brick ne compile
  probablement pas `f_hid` ; le Bluetooth est la voie principale.
- **Installation** : manuelle d'abord ; documentation Pak Store préparée ensuite.

## Règles générales

- Répondre en français formel, avec « vous » ; pas d'emojis.
- Favoriser KISS et DRY ; extraire la logique partagée plutôt que dupliquer.
- Les commentaires expliquent **pourquoi**, pas ce que le code fait déjà.
- Créer une branche dédiée avant toute modification de complexité moyenne ou
  élevée (nouvelle fonctionnalité, refactor, changement d'architecture).
- Ne jamais faire de commit directement sur `main` ou `dev` sans Pull Request.
- Ne pas faire de commit ni de push sans demande explicite.
- Les installations de paquets, suppressions de fichiers et commandes
  destructrices nécessitent une approbation humaine.
- Ne jamais citer de données personnelles (nom, prénom, e-mail, identifiant,
  téléphone, adresse) dans un commit, une branche, un tag, un titre ou une
  description de PR. Utiliser des références neutres.
- Dans OpenCode, préfixer les commandes shell avec `rtk`.
- Consulter Context7 pour la documentation des bibliothèques, SDK ou API.

## Commandes

### Build et validation

```bash
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-unknown-linux-musl

cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --target aarch64-unknown-linux-musl -p brick-controller

VERSION=0.1.0 ./apps/brick-controller/deploy/package.sh
```

Le binaire est **statique `aarch64-musl`** : aucun runtime Rust ni dépendance
dynamique n'est nécessaire sur la Brick.

### Déploiement et diagnostic

```bash
adb devices
adb shell id

# Pousser un pak en cours de développement
adb push "dist/Brick Controller.pak" "/mnt/SDCARD/Tools/tg5040/"

# Lire les logs
adb shell "cat /mnt/SDCARD/.userdata/tg5040/logs/BrickController.txt"
adb pull "/mnt/SDCARD/.userdata/tg5040/logs/BrickController.txt" .
```

Fallback SSH si ADB n'est pas disponible :

```bash
ssh -p 22 root@<ip>
# mot de passe par défaut sur tg5040 : tina
```

Utiliser le **port USB-C inférieur** pour ADB et pour le mode périphérique.
Le port supérieur est un port hôte.

## Structure cible

```text
apps/brick-controller/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── bluez.rs
│   ├── l2cap.rs
│   ├── hid.rs
│   ├── xbox.rs
│   ├── ds4.rs
│   ├── rumble.rs
│   ├── sdp.rs
│   ├── pad.rs
│   └── platform/
│       ├── mod.rs
│       ├── display.rs
│       ├── input_trimui.rs
│       ├── input_h700.rs        # plus tard
│       ├── bluetooth_init.rs    # tg5040 / tg5050
│       ├── bluetooth_systemd.rs # h700, plus tard
│       └── battery.rs
├── deploy/
│   ├── launch.sh
│   └── package.sh
└── README.md

crates/brick/                    # canvas, texte, input, png
scripts/probe.sh                 # sonde matérielle
dist/                            # archives générées
```

Le code doit rester aussi proche que possible de Padpod pour faciliter les
montées de version et conserver les crédits.

## Pak NextUI

Installation manuelle :

```text
/mnt/SDCARD/Tools/tg5040/Brick Controller.pak/
├── launch.sh
├── brick-controller
├── icon.png
├── README.txt
├── LICENSE
├── ADDITIONAL-TERMS.md
├── CREDITS.md
└── OFL.txt
```

Ne **jamais** installer dans `.system/` : ce dossier est remplacé à chaque mise
à jour NextUI.

### Variables disponibles dans `launch.sh`

- `$SDCARD_PATH` : `/mnt/SDCARD`
- `$SYSTEM_PATH` : `/mnt/SDCARD/.system/tg5040`
- `$USERDATA_PATH` : `/mnt/SDCARD/.userdata/tg5040`
- `$LOGS_PATH` : `$USERDATA_PATH/logs`
- `$PLATFORM` : `tg5040`
- `$DEVICE` : `brick` (ou `brickpro`, `smartpro`)

### Règles du `launch.sh`

- Écrire les logs dans `$LOGS_PATH/BrickController.txt`.
- Stocker l'état persistant dans `$USERDATA_PATH/brick-controller` via
  `BRICK_CONTROLLER_DATA` (fallback `PADPOD_DATA`).
- Toujours restaurer l'état à la sortie, y compris sur erreur : `trap EXIT
  INT TERM`.
- Appeler la sous-commande `restore` du binaire.
- Relancer le Bluetooth NextUI via `$SYSTEM_PATH/etc/bluetooth/bt_init.sh start`
  **uniquement si le Bluetooth était actif au lancement**.
- Faire `sync` après toute écriture sur la carte FAT.
- Ne pas laisser le rétroéclairage, le moteur ou `bluetoothd` dans un état
  intermédiaire.

## Bluetooth

### tg5040 / tg5050

- `bluetoothd` : `/usr/bin/bluetoothd`.
- Script d'init : `/etc/bluetooth/bluetoothd`.
- NextUI : `$SYSTEM_PATH/etc/bluetooth/bt_init.sh`.
- Arrêter `bluetoothd` et `bluealsa` avant la prise de contrôle.
- **Conserver `hci0` et `hciattach` actifs.**
- Lancer un `bluetoothd` dédié avec `-P input,hog` et une configuration
  temporaire.
- Restaurer avec `bt_init.sh start`, qui relance `bluetoothd`, `bluealsa`,
  l'alias et l'état discoverable/pairable.
- Écrire un marqueur dans `/tmp` pour permettre la restauration après un crash.

**Ne jamais appeler `bt_init.sh stop` complet sur `tg5040`** : cela coupe
`hciattach` et l'adaptateur, et empêche la reprise rapide. Arrêter uniquement
`bluetoothd` et `bluealsa`.

### H700 (plus tard)

- Backend séparé : systemd + `setBluetooth.sh` + `bt_init.sh`.
- Ne pas réutiliser tel quel le backend `tg5040`.
- Certains modèles H700 n'ont pas de Bluetooth : détecter et afficher un
  message clair.

## Matériel — Brick standard

Sonde validée sur NextUI `NextUI-20261013-0`, noyau `4.9.191` :

- Écran : **1024×768**, framebuffer **32 bpp BGRA**, double buffer.
- Entrées : `TRIMUI Player1` sur `/dev/input/event3`, avec `ABS_X/Y/RX/RY`
  centrés, `ABS_Z/RZ` 0..255 et `ABS_HAT0X/Y`.
- Pas de sticks analogiques.
- F1/F2 : navigation de l'app (F1 réglages, F2 moniteur d'entrées, F1+F2
  quitter). Non envoyés à l'hôte.
- Rumble : GPIO 227 + `/sys/class/motor/voltage` (3,3 V sur Brick standard),
  utilisé en mode Xbox par le rapport de sortie 0x03.
- Rétroéclairage : `/dev/disp`, non utilisé par l'app.
- Batterie : `/sys/class/power_supply/axp2202-battery/capacity`.
- Bluetooth : `bluetoothd` lancé par procd via `/etc/bluetooth/bluetoothd`,
  `bluealsa` actif, `hciattach -n ttyS1 xradio`, `hciconfig` et `btmgmt`
  disponibles, `systemctl` absent.

Règles :

- Ne pas supposer un numéro d'event (`event3` ou `event4`) : trouver le
  périphérique par nom ou par capacités.
- Ne pas supposer 32 bpp : gérer 32 bpp BGRA et 16 bpp RGB565.
- Ne pas supposer que le rumble, le rétroéclairage ou la batterie existent :
  no-op propre si absent.
- Journaliser les premiers codes evdev inconnus pour faciliter l'adaptation.

## Profils HID

- **PC** : HID générique, compatible macOS, Windows, Linux/SteamOS, Android et
  iPadOS. Profil par défaut.
- **Xbox** : identité Xbox Wireless Controller (`045E:02FD`) pour XInput sous
  Windows et la prise en charge Xbox native sur macOS, iPadOS et Android.
- Le profil est stocké **par hôte** dans `$BRICK_CONTROLLER_DATA/profiles`
  (`ADDR=pc` ou `ADDR=xbox`). L'identité réellement utilisée à la première
  connexion est mémorisée pour cet hôte.
- Le dernier profil utilisé est conservé dans `$BRICK_CONTROLLER_DATA/mode` :
  il sert de valeur par défaut à un hôte inconnu et survit à un redémarrage
  avant le réappairage.
- Changer le profil de l'hôte **actuellement utilisé** oublie son appairage
  BlueZ, efface l'hôte mémorisé et redémarre la session Bluetooth avec la
  nouvelle identité ; l'hôte doit supprimer l'ancien appairage puis réappairer.
- Changer le profil d'un autre hôte enregistre le choix ; il sera appliqué à la
  prochaine connexion de cet hôte. L'ancien appairage doit être supprimé sur
  l'hôte avant de le reconnecter.
- Le nom Bluetooth, le descripteur HID, le DeviceID et l'alias de l'adaptateur
  suivent le profil : « Brick Controller » ou « Xbox Wireless Controller ».
- En mode Xbox, les rapports de sortie 0x03 reçus sur le canal interrupt ou par
  SET_REPORT pilotent le moteur via `rumble.rs` (GPIO 227). Le mode PC n'a pas
  de rapport de sortie : aucune vibration.
- Start et Select sont envoyés à l'hôte comme boutons de manette.
- F1 ouvre les réglages (liste des hôtes appairés) ; F2 ouvre le moniteur
  d'entrées ; F1+F2 maintenu 3 s quitte l'application. Pendant le maintien,
  le pied de page affiche « keep holding to quit… ».
- Dans les réglages : D-pad pour choisir, A pour connecter/déconnecter, F2 pour
  basculer le profil de l'hôte sélectionné. Aucune action B « back ». Quand un
  hôte est connecté, F1 revient à la vue controller. Les rapports HID sont
  neutralisés pendant l'ouverture des réglages ou du moniteur.
- La vue controller n'est affichée que lorsqu'un hôte est connecté ; sans
  connexion, l'application reste sur la vue réglages.
- Un changement de mode (PC/Xbox) reste sur la vue réglages, y compris quand
  la session Bluetooth redémarre avec la nouvelle identité.
- Le moniteur d'entrées ne se ferme qu'avec F2 ; le libellé de l'action est
  « controller ».
- Sans hôte appairé, les réglages affichent une ligne **Next pairing** : F2
  choisit l'identité que verra le prochain hôte appairé.
- Menu envoie Home/Guide à l'hôte ; l'appui long n'a plus d'action.
- Sur Brick standard, les sticks sont absents : le descripteur les déclare
  centrés pour rester compatible.

Hôtes : macOS, Windows, Linux/SteamOS, Android, iPadOS. Un hôte mémorise
l'identité de la manette au premier appairage ; après un changement de profil,
il faut supprimer l'ancien appairage sur l'hôte et réappairer. Le profil choisi
reste dans `profiles`, mais l'appairage BlueZ de l'hôte courant est supprimé
pour forcer un réappairage propre.

## Validation

Après toute modification de code source :

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo build --release --target aarch64-unknown-linux-musl -p brick-controller`
5. packaging du `.pak`
6. test sur la Brick

Si une image Docker est utilisée pour l'outillage ou le firmware, la
reconstruire avant le test.

### Tests unitaires

- descripteurs HID et encodage des rapports ;
- SDP et canaux L2CAP ;
- mapping des boutons et des axes ;
- restauration Bluetooth et chemins de secours.

### Tests sur matériel

- appairage macOS, Windows, Linux ;
- tous les boutons, D-pad, gâchettes, Start/Select, Menu ;
- Menu : appui simple = Home/Guide, appui long sans effet ;
- F1 pour les réglages, F2 pour le moniteur d'entrées, F1+F2 pour quitter ;
- maintien F1+F2 : message « keep holding to quit… » puis sortie ;
- F2 seul ferme le moniteur, libellé « controller » ;
- vue réglages par défaut sans hôte, vue controller seulement connecté ;
- changement de profil : la vue réglages reste affichée ;
- aucune action B « back » dans les réglages ni le moniteur ;
- défilement du moniteur d'entrées et affichage de la dernière commande ;
- bascule PC/Xbox d'un hôte, redémarrage de session et réappairage ;
- vibrations en mode Xbox : rapport 0x03 sur le canal interrupt et SET_REPORT,
  moteur arrêté à la déconnexion et à la sortie ;
- connexion d'un hôte avec le profil mémorisé ;
- reconnexion au dernier hôte ;
- sortie propre et restauration NextUI ;
- crash de l'application et récupération ;
- Bluetooth désactivé au lancement ;
- audio Bluetooth connecté avant la session ;
- mise à jour NextUI puis nouveau test.

Logs utiles : `$LOGS_PATH/BrickController.txt`, `dmesg`, `btmon`,
`hcidump`, `logread`.

## Anti-patterns

| Anti-pattern | Conséquence | Bon usage |
|---|---|---|
| `std::process::exit` avant la restauration | Bluetooth/rétroéclairage laissés cassés | `Drop`, `restore`, `trap EXIT` |
| Appeler `bt_init.sh stop` complet sur tg5040 | `hciattach` tué, adaptateur indisponible | Arrêter seulement `bluetoothd`/`bluealsa` |
| Tuer `hci0` ou `hciattach` | Impossible de reconnecter sans redémarrage | Conserver l'adaptateur actif |
| Coder `/dev/input/event3` en dur | Mauvais périphérique selon le boot | Chercher par nom/capacités |
| Supposer 32 bpp | Écran noir ou décalé | Gérer 16 et 32 bpp |
| Supposer des sticks | UI incohérente sur Brick standard | Détecter les axes |
| `unwrap()` sur un chemin matériel optionnel | Crash sur un modèle différent | `Option`/no-op |
| Modifier `.system/` | Écrasé à la mise à jour NextUI | Utiliser `Tools/tg5040/` |
| Copier Padpod sans licence/crédits | Violation GPL-3 | Garder LICENSE, credits et source |
| Tester uniquement sur l'hôte | Bugs de framebuffer/entrées non vus | Tester sur la Brick |
| Oublier `sync` après écriture FAT | État corrompu après coupure | `sync` dans le cleanup |

## Sécurité et récupération

- Toujours vérifier après la sortie que `bluetoothd` et `bluealsa` sont dans
  l'état attendu.
- Ne jamais laisser la Brick avec le Bluetooth arrêté.
- Ne jamais écrire dans `.system/`.
- Ne jamais supprimer un fichier ou un volume sans accord humain.
- Ne jamais exécuter de commande destructive sans confirmation.
- En cas de crash, le `launch.sh` doit pouvoir restaurer l'état via le marqueur
  `/tmp` et la sous-commande `restore`.
- Une coupure de courant ne doit pas laisser de modification persistante
  empêchant NextUI de redémarrer normalement.

## Licence et crédits

- Padpod : **GPL-3.0 avec clauses additionnelles**.
- Conserver `LICENSE`, `ADDITIONAL-TERMS.md`, `CREDITS.md` et `OFL.txt`.
- Conserver les lignes de copyright dans les fichiers.
- Renommer l'application : « Brick Controller », binaire `brick-controller`,
  jamais « Padpod ».
- Indiquer qu'il s'agit d'une version modifiée.
- Afficher le crédit d'auteur et le lien Padpod discrètement dans l'écran
  Réglages, avec `NAME VERSION` ; ne pas les afficher sur l'écran principal.
  Conserver `LICENSE`, `ADDITIONAL-TERMS.md` et `CREDITS.md`.
- Publier la source complète sous les mêmes termes.
- Le code repris de deck-as-controller (MIT) conserve ses notices MIT.
- NextUI est sous PolyForm Noncommercial : le pak reste un travail séparé ; ne
  pas copier de code du cœur NextUI dans le pak sans vérifier la licence.

## Documentation

Le README doit couvrir :

- installation manuelle du `.pak` ;
- activation du Bluetooth dans NextUI ;
- appairage macOS/Windows/Linux/Android/iPadOS ;
- profils PC et Xbox par hôte ;
- limite « Bluetooth uniquement » ;
- absence de sticks/gyro/trackpads sur Brick standard ;
- partage d'antenne Wi-Fi/Bluetooth ;
- audio Bluetooth indisponible pendant la session ;
- procédure de récupération si le Bluetooth reste arrêté ;
- crédits et licence.

Un `pak.json` sera ajouté plus tard pour le Pak Store, avec `name`, `version`,
`type: "TOOL"`, `description`, `author`, `repo_url`, `release_filename`,
`platforms: ["tg5040"]`, `screenshots` et `changelog`.

## Suivi de tâche

Si une tâche Jira est mentionnée :

1. passer la tâche à « En cours » avant la première modification ;
2. après validation complète, passer à « implémenté » ;
3. après merge et push dans `dev`/`main`, passer à « Validé ».

Ne pas coder en dur un ID de transition : lire les transitions disponibles.
Ne pas rétrograder une tâche. Ne pas créer de ticket sans demande explicite.
Le merge et le push restent soumis à approbation humaine.

## Roadmap

- **Phase 1** : Brick standard `tg5040`, Bluetooth, profils PC et Xbox par
  hôte.
- **Phase 2** : Brick Pro et Smart Pro.
- **Phase 3** : Smart Pro S `tg5050`.
- **Phase 4** : H700, si nécessaire.
- **Recherche séparée** : USB HID (`f_hid`, FunctionFS ou module noyau).
