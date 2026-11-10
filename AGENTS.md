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
- **Navigation de l'app** : écran **Devices** comme accueil sans hôte. F1 ouvre
  l'écran Devices depuis la vue controller ; F1+F2 maintenu 1,5 s l'ouvre aussi, avec HID
  neutralisé pendant le maintien. Dans les écrans d'interface : D-pad, A, B
  (F1 = retour, F2 = valider). Le jeu de boutons est neutralisé vers l'hôte
  pendant ces écrans. Start et Select restent des boutons de manette.
  Dans l'écran Devices, **B** quitte immédiatement l'application, sans confirmation.
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
│   ├── display.rs
│   ├── rumble.rs
│   ├── sdp.rs
│   ├── pad.rs
│   └── platform/
│       ├── mod.rs
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
- Restaurer l'écran et les réglages NextUI avec
  `$SYSTEM_PATH/bin/syncsettings.elf` à la sortie, quand il est disponible.
- Basculer le gouverneur CPU en `powersave` avec
  `$SYSTEM_PATH/bin/governor.sh` ; NextUI restaure son propre gouverneur à la
  sortie du Pak.
- Relancer le Bluetooth NextUI via `$SYSTEM_PATH/etc/bluetooth/bt_init.sh start`
  **uniquement si le Bluetooth était actif au lancement**.
- Faire `sync` après toute écriture sur la carte FAT.
- Garantir une seule instance : arrêter l'instance précédente, attendre la fin
  de son cleanup, puis démarrer. Le PID du wrapper est conservé dans
  `/tmp/brick-controller-launch.pid` et supprimé à la sortie.
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
- F1/F2 : touches de navigation de l'app (F1 menu/retour, F2 validation,
  F1+F2 maintenu 1,5 s pour l'écran Devices). Non envoyées à l'hôte. Dans les écrans
  d'interface, le D-pad et A/B sont aussi utilisés et neutralisés côté hôte.
- Rumble : GPIO 227 + `/sys/class/motor/voltage` (3,3 V sur Brick standard),
  utilisé en mode Xbox par le rapport de sortie 0x03.
- Rétroéclairage : `/dev/disp`, ioctl `0x102` pour régler la luminosité brute
  et `0x103` pour la relire. `0` éteint la dalle sans arrêter le framebuffer ni
  le Bluetooth. La valeur est mémorisée dans
  `$BRICK_CONTROLLER_DATA/screen-off` avant extinction, puis restaurée à la
  sortie ou par `restore` après un crash.
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
  (`ADDR=pc` ou `ADDR=xbox`). L'identité utilisée au premier appairage est
  mémorisée pour cet hôte et réutilisée pour le reconnecter.
- Le dernier profil utilisé est conservé dans `$BRICK_CONTROLLER_DATA/mode` :
  il sert de valeur par défaut au prochain appairage et survit à un
  redémarrage avant le réappairage.
- Le profil d'un hôte **ne se modifie plus depuis Devices**. Le type PC/Xbox se
  choisit uniquement lors d'un nouvel appairage, via l'écran Pairing puis X.
  Pour changer le type d'un hôte existant, l'oublier sur la Brick et sur
  l'hôte, choisir l'autre type dans l'écran d'appairage, puis réappairer.
- Le nom Bluetooth, le descripteur HID, le DeviceID et l'alias de l'adaptateur
  suivent le profil : « Brick Controller » ou « Xbox Wireless Controller ».
- En mode Xbox, les rapports de sortie 0x03 reçus sur le canal interrupt ou par
  SET_REPORT pilotent le moteur via `rumble.rs` (GPIO 227). Le mode PC n'a pas
  de rapport de sortie : aucune vibration.
- Start et Select sont envoyés à l'hôte comme boutons de manette.
- L'écran **Devices** est l'écran d'accueil. Il liste toujours les hôtes appairés
  puis **Pair a new device**, même lorsqu'un hôte est connecté. Son footer
  propose **B Quit**, **X Forget** et **A Connect**. Oublier un hôte demande
  toujours confirmation. Il n'y a plus d'écran de détail ni de changement de
  type PC/Xbox pour un hôte déjà appairé.
- Aucune connexion n'est automatique : au lancement, aucune adresse enregistrée
  n'est contactée. Ouvrir Devices coupe la connexion en cours, la tentative de
  connexion et l'appairage en cours (cible du reconnecteur effacée, discoverable
  coupé, canal semi-ouvert libéré). **Connect** reste une action explicite.
- F1 ouvre l'écran Devices depuis la vue controller. F1+F2 maintenu 1,5 s l'ouvre
  aussi : aucun rapport HID n'est envoyé pendant le maintien. Un overlay
  « Keep holding to open devices… » montre la progression.
- Dans l'écran Devices, la navigation est celle de NextUI : D-pad, **A** pour
  connecter l'hôte sélectionné (ou ouvrir **Pair a new device**) et **X** pour
  l'oublier. Dans un sous-écran, B revient et A valide. Dans l'écran Devices, B
  quitte immédiatement l'application. F1 est un alias de retour et F2 de
  validation. Les rapports HID sont neutralisés.
- Seul l'écran Pairing rend l'adaptateur discoverable et accepte les canaux
  entrants ; Devices et ses sous-écrans les refusent.
- Au retour à la vue controller, les rapports restent neutres jusqu'au
  relâchement de toutes les touches utilisées dans l'interface.
- Le choix du type de contrôleur se fait uniquement lors de l'appairage : dans
  l'écran Pairing, X ouvre la liste PC/Xbox pour le prochain appairage.
  Choisir un autre type redémarre la session Bluetooth avec la nouvelle
  identité et réaffiche Devices. Aucun changement de type n'est proposé pour un
  hôte déjà appairé.
- **Pair a new device** reste disponible même avec un hôte connecté ; il ouvre
  l'écran d'appairage, où X permet de changer l'identité du prochain appairage.
- À la connexion d'un hôte, la vue controller affiche l'hôte, le profil et les
  derniers évènements de touches. Un compte à rebours de 10 s est affiché.
  Après 10 s, le rétroéclairage passe à 0 ; les rapports HID continuent. Un
  maintien de F1 ou F2 rallume l'écran sans déclencher l'action de la touche.
- Ouvrir l'écran Devices annule l'extinction programmée et coupe la session en
  cours.
- Le bouton Menu envoie Home/Guide à l'hôte depuis la vue controller ; dans les écrans
  d'interface, il est neutralisé.
- Sur Brick standard, les sticks sont absents : le descripteur les déclare
  centrés pour rester compatible.

Hôtes : macOS, Windows, Linux/SteamOS, Android, iPadOS. Un hôte mémorise
l'identité de la manette au premier appairage et le Brick la réutilise pour le
reconnecter. Pour changer cette identité, il faut oublier l'hôte des deux
côtés, choisir l'autre type lors d'un nouvel appairage, puis réappairer.

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
- Menu (bouton) : appui simple = Home/Guide, appui long sans effet ;
- F1 ouvre l'écran Devices ; F1+F2 maintenu 1,5 s l'ouvre aussi ;
- maintien F1+F2 : overlay « Keep holding to open devices… », HID neutralisé ;
- navigation Devices/sous-écrans avec D-pad/A/B ; F1 retour, F2 validation ;
- B dans l'écran Devices quitte immédiatement, sans confirmation ;
- Écran Devices par défaut sans hôte, sans connexion automatique ;
- Pair a new device reste listé et sélectionnable avec un hôte connecté ;
- retour sur Devices : connexion en cours et appairage arrêtés ;
- A Connect direct sur l'hôte sélectionné, avec le profil mémorisé ;
- X Forget dans Devices : confirmation, oubli de l'hôte, retour à la liste ;
- choix PC/Xbox uniquement dans l'écran Pairing, puis réappairage complet ;
- aucune fuite de D-pad/A/B/Menu/Start/Select vers l'hôte dans l'interface ;
- compte à rebours de 10 s, extinction, rallumage par maintien F1 ou F2 ;
- vue controller : derniers évènements de touches et profil ;
- choix PC/Xbox lors d'un nouvel appairage, redémarrage de session et réappairage ;
- vibrations en mode Xbox : rapport 0x03 sur le canal interrupt et SET_REPORT,
  moteur arrêté à la déconnexion et à la sortie ;
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
| Éteindre `/dev/disp` sans mémoriser la luminosité | Écran noir après un crash | Fichier `screen-off`, `restore`, `syncsettings.elf` |
| Laisser deux instances du Pak | `Address in use`, écran Bluetooth trompeur, entrées partagées | Verrou PID dans `launch.sh`, arrêt de l'instance précédente |

## Sécurité et récupération

- Toujours vérifier après la sortie que `bluetoothd` et `bluealsa` sont dans
  l'état attendu.
- Ne jamais laisser la Brick avec le Bluetooth arrêté.
- Ne jamais écrire dans `.system/`.
- Ne jamais supprimer un fichier ou un volume sans accord humain.
- Ne jamais exécuter de commande destructive sans confirmation.
- En cas de crash, le `launch.sh` doit pouvoir restaurer l'état via le marqueur
  `/tmp` et la sous-commande `restore`.
- Vérifier après la sortie que l'écran n'est pas resté éteint : `restore`
  applique la luminosité mémorisée, puis `syncsettings.elf` restaure celle de
  NextUI.
- Une coupure de courant ne doit pas laisser de modification persistante
  empêchant NextUI de redémarrer normalement.

## Licence et crédits

- Padpod : **GPL-3.0 avec clauses additionnelles**.
- Conserver `LICENSE`, `ADDITIONAL-TERMS.md`, `CREDITS.md` et `OFL.txt`.
- Conserver les lignes de copyright dans les fichiers.
- Renommer l'application : « Brick Controller », binaire `brick-controller`,
  jamais « Padpod ».
- Indiquer qu'il s'agit d'une version modifiée.
- Conserver le crédit d'auteur et le lien Padpod dans `CREDITS.md` et les
  fichiers du pak ; ne pas les afficher sur l'écran principal.
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
