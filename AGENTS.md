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
- **Profils HID** : PC (HID générique), Xbox (identité Xbox Wireless
  Controller), PlayStation (identité DualShock 4) et Switch (Switch Pro
  Controller), choisis **lors de l'appairage**.
- **Navigation de l'app** : écran **Devices** comme accueil sans hôte.
  Menu+Start maintenu 1 s déconnecte et ouvre Devices depuis la vue Connected,
  avec HID neutralisé pendant le maintien ; un appui simple Menu+Start rallume
  l'écran éteint. F1/F2 ne naviguent pas dans l'interface : elles sont mappées
  sur L3/R3 et envoyées à l'hôte. Dans les écrans d'interface : D-pad, A, B. Le
  jeu de boutons est neutralisé vers l'hôte pendant ces écrans. Start et Select
  restent des boutons de manette.
  Dans l'écran Devices, **B** quitte immédiatement l'application, sans confirmation.
- **USB HID** : hors périmètre du MVP. Le noyau stock de la Brick ne compile
  probablement pas `f_hid` ; le Bluetooth est la voie principale.
- **Installation** : manuelle d'abord ; documentation Pak Store préparée ensuite.

## Règles générales

- Répondre en français formel, avec « vous » ; pas d'emojis.
- Favoriser KISS et DRY ; extraire la logique partagée plutôt que dupliquer.
- Respecter l'architecture de la section **Architecture** : toute nouvelle
  logique de profil va dans `profiles/`, jamais dans `main.rs`.
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

### Diagnostic rust-analyzer

Si l'éditeur affiche des erreurs en cascade sur `#[derive(...)]`, `format!` ou
`include_bytes!`, rust-analyzer n'a probablement pas trouvé `rustc` : son
processus n'hérite pas toujours du `PATH` du shell. Vérifier que
`$HOME/.cargo/bin` est dans le `PATH` du serveur LSP, ou renseigner
`cargo.extraEnv.PATH` et `server.extraEnv.PATH` dans les réglages de l'éditeur.
La référence reste `cargo check --workspace --all-targets`.

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

## Architecture

Les profils, l'état d'entrée, le transport Bluetooth, la plateforme, la
session et l'interface sont désormais séparés en modules. L'architecture
ci-dessous est la **cible normative** pour toute nouvelle évolution. Chaque
étape doit compiler et passer les tests.

### Principes

- **Cœur pur, périphérie impérative** : `profiles/`, `input.rs` et `hid.rs` ne
  font aucune E/S, ne lisent pas l'horloge et n'impriment rien. Ils transforment
  un état en octets et des octets en actions ; la session fournit le temps et
  les canaux.
- **Ensemble fermé de profils** : un `enum` et un dispatch statique. Pas de
  `Box<dyn Trait>` ni de registre dynamique tant que les profils sont connus à
  la compilation.
- **Un fichier par protocole** : identité, descripteur, encodage des rapports,
  décodage des sorties et tests du profil vivent dans le même module.
- **Métadonnées séparées de l'état** : `ProfileSpec` décrit l'identité ;
  `Codec` porte l'état nécessaire aux rapports (compteur, horodatage, file de
  réponses Switch).
- **Dépendances orientées** : `profiles` ne connaît ni BlueZ, ni L2CAP, ni
  l'interface. `ui` ne connaît pas `Profile`.
- **Interfaces étroites** : `pub(crate)` par défaut ; un module exporte
  uniquement ce que son parent utilise.
- **Tests au plus près du code** : tests unitaires dans chaque module, plus une
  suite de conformité commune à tous les profils.

### Arborescence cible

Le style retenu est `foo.rs` + dossier `foo/`, sans fichiers `mod.rs`.

```text
apps/brick-controller/
├── Cargo.toml
├── src/
│   ├── main.rs                     # bootstrap, sous-commande restore
│   ├── app.rs                      # boucle applicative, réexports
│   ├── app/
│   │   ├── session.rs              # machine à états d'une session Bluetooth
│   │   ├── settings.rs             # hôtes, profil par hôte, mode, hôte courant
│   │   ├── tests.rs                # tests de navigation et aperçus
│   │   └── view_model.rs           # Screen -> ui::View
│   ├── profiles.rs                 # Profile, ProfileSpec, Controller
│   ├── profiles/
│   │   ├── spec.rs                 # métadonnées statiques et registre ALL
│   │   ├── codec.rs                # trait Codec, AnyCodec, Reports, Output
│   │   ├── generic_hid.rs          # profil PC
│   │   ├── xbox.rs                 # Xbox Wireless Controller
│   │   ├── dualshock4.rs           # PlayStation, identité DualShock 4
│   │   └── switch_pro.rs           # Switch Pro Controller
│   ├── hid.rs                      # transactions et trames HID communes
│   ├── input.rs                    # PadState, Button, Hat, état neutre
│   ├── bluetooth.rs                # réexports du transport
│   ├── bluetooth/
│   │   ├── address.rs              # Address partagée
│   │   ├── bluez.rs                # D-Bus BlueZ
│   │   ├── l2cap.rs                # sockets L2CAP
│   │   └── sdp.rs                  # enregistrement SDP
│   ├── platform.rs
│   ├── platform/
│   │   ├── display.rs              # rétroéclairage
│   │   ├── rumble.rs               # moteur
│   │   ├── status.rs               # batterie, charge, Wi-Fi, horloge
│   │   ├── input_trimui.rs         # evdev TrimUI
│   │   ├── input_h700.rs           # phase 4, prévu
│   │   └── bluetooth_init.rs       # tg5040 / tg5050, prévu
│   ├── ui.rs                       # réexports, layout et primitives de dessin
│   ├── ui/
│   │   ├── theme.rs                # Theme, couleurs et nextval.elf
│   │   ├── widgets.rs              # lignes, footers, modales, barre de chargement
│   │   ├── screens.rs              # rendu des View
│   │   ├── renderer.rs             # thread de rendu, animation
│   │   └── preview.rs              # aperçus, uniquement en test
│   └── test_support.rs             # analyse de descripteur partagée par les tests
├── deploy/
│   ├── launch.sh
│   └── package.sh
└── README.md

crates/brick/                    # canvas, texte, input, png
scripts/probe.sh                 # sonde matérielle
dist/                            # archives générées
```

Le code doit rester aussi proche que possible de Padpod pour faciliter les
montées de version et conserver les crédits. Le déplacement des profils dans
`profiles/` est une réorganisation : la logique de Padpod, ses commentaires de
protocole et ses notices doivent être conservés, pas réécrits.

### Responsabilités des modules

| Module | Rôle | Ne doit pas |
|---|---|---|
| `main.rs` | bootstrap, `restore`, appel de `app::run` | contenir de la logique de profil |
| `app/session.rs` | machine à états, lien Bluetooth, extinction, filtrage des entrées | connaître les détails d'un protocole |
| `app/settings.rs` | hôtes, profil par hôte, `mode`, hôte courant | lire l'écran ou le pad |
| `app/view_model.rs` | `Screen`/`Settings` -> `ui::View` | dessiner |
| `profiles/spec.rs` | `Profile`, `ProfileSpec`, `ALL`, clés, DeviceID | encoder un rapport |
| `profiles/codec.rs` | `Codec`, `AnyCodec`, `Controller`, `Reports`, `Output` | faire des E/S |
| `profiles/<protocole>.rs` | identité, descripteur, rapports, sorties, tests | importer `bluez`, `l2cap`, `ui` ou `app` |
| `hid.rs` | transactions, trames DATA, handshake commun | connaître une identité de manette |
| `input.rs` | `PadState`, `Button`, `Hat`, état neutre | lire evdev |
| `bluetooth/*` | BlueZ, L2CAP, SDP, `Address` | connaître les profils |
| `platform/*` | écran, moteur, batterie, thème, evdev | connaître les profils |
| `ui/*` | thème, widgets, écrans, rendu, aperçus | connaître `Profile` ou `Settings` |
| `test_support.rs` | analyse de descripteur, helpers de test | être compilé hors des tests |

### Contrat d'un profil

```rust
pub struct ProfileSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub bluetooth_name: &'static str,
    pub usb: Option<UsbId>,
    pub descriptor: &'static [u8],
    pub schedule: Schedule,
    pub rumble: bool,
}

pub enum Schedule {
    OnChange { min_gap: Duration },
    Periodic { every: Duration },
}

pub trait Codec {
    fn packets(&mut self, state: &PadState, elapsed: Duration) -> Reports;
    fn control(&mut self, message: &[u8], link: &LinkContext, state: &PadState) -> hid::Control;
    fn output(&mut self, report: &[u8], link: &LinkContext) -> Output;
}

pub struct LinkContext {
    pub local: Address,
    pub peer: Address,
}

pub struct Output {
    pub rumble: Option<u8>,
    pub disconnect: bool,
}
```

- `Profile` est l'identité stable et sérialisée (`pc`, `xbox`, `ps4`,
  `switch`). `ProfileSpec` porte les métadonnées immuables. `Codec` porte
  l'état du protocole.
- `AnyCodec` est l'`enum` de dispatch :

  ```rust
  enum AnyCodec {
      Pc(generic_hid::Codec),
      Xbox(xbox::Codec),
      PlayStation(dualshock4::Codec),
      Switch(switch_pro::Codec),
  }
  ```

- `Controller` est la façade utilisée par `app/session.rs` :

  ```rust
  pub struct Controller {
      profile: Profile,
      codec: AnyCodec,
  }

  impl Controller {
      pub fn new(profile: Profile) -> Self;
      pub fn spec(&self) -> &'static ProfileSpec;
      pub fn packets(&mut self, state: &PadState, elapsed: Duration) -> Reports;
      pub fn control(&mut self, message: &[u8], link: &LinkContext, state: &PadState) -> hid::Control;
      pub fn output(&mut self, report: &[u8], link: &LinkContext) -> Output;
  }
  ```

- `packets` renvoie un ou deux rapports. Le deuxième sert au rapport Guide de
  Xbox, séparé du rapport principal.
- `Reports` doit éviter les allocations sur le chemin chaud : capacité fixe,
  premier rapport et second optionnel.
- `output` traite les rapports de sortie : rumble, sous-commandes Switch,
  demande de déconnexion. Un même rapport peut demander du rumble et une
  déconnexion.
- Aucun profil ne connaît `Channel`, `Link`, `bluez` ou `ui`.

### Règles de dépendances

```text
main -> app
app  -> profiles, bluetooth, platform, ui, input
profiles -> input, hid, bluetooth::address
bluetooth -> address
ui -> platform::status (données uniquement)
platform -> OS
hid -> std
input -> std
```

- `profiles` n'importe jamais `bluez`, `l2cap`, `ui`, `app` ou `platform`.
- `hid` n'importe jamais `profiles`.
- `ui` ne connaît pas `Profile` ; `app::view_model` lui passe des libellés et
  des lignes.
- `bluetooth` ne connaît pas les profils ; `app` lui passe le nom et le
  descripteur.
- Aucun cycle de dépendances. Aucun module `utils.rs` ou `common.rs`.

### Ajouter un profil

1. Créer `profiles/<protocole>.rs` avec `SPEC`, le descripteur, `Codec` et ses
   tests.
2. Ajouter la variante dans `Profile`, l'arm de `spec()` et l'entrée dans
   `Profile::ALL`.
3. Ajouter la variante dans `AnyCodec` et les délégations.
4. Passer la suite de conformité.
5. Mettre à jour `README.md` et `CREDITS.md`.

Aucune modification de `app/session.rs`, de l'interface ou du transport ne doit
être nécessaire. C'est le critère de réussite de l'architecture.

### Migration

Le refactor se fait dans cet ordre, chaque étape étant compilable et testée :

1. **fait** — extraction de `input.rs`, `hid.rs` allégé et `profiles/` ;
2. **fait** — introduction de `ProfileSpec`, `Profile`, `Codec` et
   `Controller` ;
3. **fait** — câblage du profil PlayStation (DualShock 4) ;
4. **fait** — découpage de `main.rs` en `app/session.rs`, `app/settings.rs` et
   `app/view_model.rs` ;
5. **fait** — ajout du profil Switch dans `profiles/switch_pro.rs` ;
6. **fait** — découpage de `ui.rs` en `ui/theme.rs`, `ui/widgets.rs`,
   `ui/screens.rs`, `ui/renderer.rs` et `ui/preview.rs`.

Ne pas ajouter de nouveau `match profile` dans `main.rs` : la façade
`Controller` est le seul point d'entrée. Les nouveaux profils suivent la
procédure ci-dessus.

### Commentaires et documentation

- Chaque fichier commence par un `//!` qui dit sa responsabilité, sa frontière
  et, pour un profil, la source du descripteur et du protocole.
- Les fonctions non triviales documentent le contrat, les invariants et le
  pourquoi ; pas de commentaire qui répète la ligne suivante.
- Les valeurs de protocole (IDs de rapport, tailles, CRC, délais) sont
  commentées avec leur origine ou leur unité.
- Un commentaire qui décrit une cible non encore implémentée doit le dire
  explicitement (« cible », « pas encore déclaré »).
- Les commentaires de protocole repris de Padpod ou de deck-as-controller sont
  conservés et crédités, pas réécrits.

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
- F1/F2 : touches de manette mappées sur L3/R3, jamais utilisées pour la
  navigation de l'app. Menu+Start maintenu 1 s déconnecte et ouvre Devices ; le
  même combo, appuyé une fois, rallume l'écran. Dans les écrans d'interface, le
  D-pad et A/B sont utilisés et neutralisés côté hôte.
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
- **PlayStation** : identité DualShock 4 (`054C:09CC`), nom Bluetooth
  « Wireless Controller », module `profiles/dualshock4.rs`. Il n'y a pas
  d'identité DualSense distincte : le profil PlayStation présente une manette
  DualShock 4.
- **Switch** : identité Switch Pro Controller (`057E:2009`), nom Bluetooth
  « Pro Controller », module `profiles/switch_pro.rs`. Le handshake Switch
  (sous-commandes 0x01, réponses 0x21) est porté de `deck-as-controller` (MIT)
  avec conservation de la notice. Pas d'IMU, NFC, IR ni HD rumble.
- Le profil est stocké **par hôte** dans `$BRICK_CONTROLLER_DATA/profiles`
  (`ADDR=pc`, `ADDR=xbox`, puis `ADDR=ps4` ou `ADDR=switch`). L'identité
  utilisée au premier appairage est mémorisée pour cet hôte et réutilisée pour
  le reconnecter.
- Le dernier profil utilisé est conservé dans `$BRICK_CONTROLLER_DATA/mode` :
  il sert de valeur par défaut au prochain appairage et survit à un
  redémarrage avant le réappairage.
- Le profil d'un hôte **ne se modifie plus depuis Devices**. Le type de
  contrôleur se choisit uniquement lors d'un nouvel appairage, via l'écran
  Pairing puis X. Pour changer le type d'un hôte existant, l'oublier sur la
  Brick et sur l'hôte, choisir l'autre type dans l'écran d'appairage, puis
  réappairer.
- Le nom Bluetooth, le descripteur HID, le DeviceID (quand il existe) et
  l'alias de l'adaptateur suivent le profil : « Brick Controller », « Xbox
  Wireless Controller », « Wireless Controller » ou « Pro Controller ».
- Les profils Xbox, PlayStation et Switch pilotent le moteur via
  `platform/rumble.rs` (GPIO 227) à partir de leurs rapports de sortie
  respectifs. Le mode PC n'a pas de rapport de sortie : aucune vibration.
- Start et Select sont envoyés à l'hôte comme boutons de manette.
- L'écran **Devices** est l'écran d'accueil. Il liste toujours les hôtes appairés
  puis **Pair a new device**, même lorsqu'un hôte est connecté. Son footer
  propose **B Quit**, **X Forget** et **A Connect**. Oublier un hôte demande
  toujours confirmation. Il n'y a plus d'écran de détail ni de changement de
  type de contrôleur pour un hôte déjà appairé.
- Aucune connexion n'est automatique : au lancement, aucune adresse enregistrée
  n'est contactée. Ouvrir Devices coupe la connexion en cours, la tentative de
  connexion et l'appairage en cours (cible du reconnecteur effacée, discoverable
  coupé, canal semi-ouvert libéré). **Connect** reste une action explicite.
- **A Connect** affiche immédiatement une popup bloquante « Connecting to
  <hôte> » avec une barre de chargement en color1 et le texte en color4. Elle
  disparaît à la connexion, ou au bout de 15 s si l'hôte ne répond pas (la
  cible est alors effacée).
- Un seul composant de barre de chargement est utilisé partout, avec color1
  comme couleur principale : mode déterminé pour le maintien de déconnexion
  (1 s), mode indéterminé animé pour le splash et la connexion.
- Menu+Start maintenu 1 s déconnecte et ouvre Devices depuis la vue Connected :
  aucun rapport HID n'est envoyé pendant le maintien. Un overlay
  « Keep holding to disconnect… » montre la progression. F1/F2 ne sortent pas
  de cette vue ; elles restent des boutons L3/R3 pour l'hôte.
- Dans l'écran Devices, la navigation est celle de NextUI : D-pad, **A** pour
  connecter l'hôte sélectionné (ou ouvrir **Pair a new device**) et **X** pour
  l'oublier. Dans un sous-écran, B revient et A valide. Dans l'écran Devices, B
  quitte immédiatement l'application. Les rapports HID sont neutralisés.
- Seul l'écran Pairing rend l'adaptateur discoverable et accepte les canaux
  entrants ; Devices et ses sous-écrans les refusent.
- Au retour à la vue controller, les rapports restent neutres jusqu'au
  relâchement de toutes les touches utilisées dans l'interface.
- Le choix du type de contrôleur se fait uniquement lors de l'appairage : dans
  l'écran Pairing, X ouvre la liste des profils pour le prochain appairage.
  Choisir un autre type redémarre la session Bluetooth avec la nouvelle
  identité et réaffiche Devices. Aucun changement de type n'est proposé pour un
  hôte déjà appairé.
- **Pair a new device** reste disponible même avec un hôte connecté ; il ouvre
  l'écran d'appairage, où X permet de changer l'identité du prochain appairage.
- À la connexion d'un hôte, la vue controller est titrée **Connected** et
  affiche l'hôte, le profil et la consigne d'extinction. Elle ne liste plus les
  évènements de touches. Après 10 s, le rétroéclairage passe à 0 ; les rapports
  HID continuent. Un appui simple Menu+Start rallume l'écran sans envoyer le
  combo à l'hôte et réarme le cycle de 10 s.
- Ouvrir l'écran Devices annule l'extinction programmée et coupe la session en
  cours.
- Le bouton Menu seul envoie Home/Guide à l'hôte depuis la vue controller ;
  Menu+Start est réservé au réveil et à la déconnexion. Dans les écrans
  d'interface, Menu est neutralisé.
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
- suite de conformité `Profile::ALL` : clé unique et round-trip, descripteur
  non vide, longueur des paquets conforme au descripteur, paquet neutre,
  cohérence du profil ;
- analyseur de descripteur partagé dans `test_support.rs`, jamais recopié dans
  chaque profil ;
- SDP et canaux L2CAP ;
- sous-commandes Switch, réponses 0x21 et lecture SPI ;
- mapping des boutons et des axes ;
- restauration Bluetooth et chemins de secours.

### Tests sur matériel

- appairage macOS, Windows, Linux ;
- tous les boutons, D-pad, gâchettes, Start/Select, Menu ;
- Menu (bouton) : appui simple = Home/Guide, appui long sans effet ;
- Menu+Start maintenu 1 s déconnecte et ouvre Devices ;
- F1/F2 sans navigation, envoyées à l'hôte comme L3/R3 ;
- maintien Menu+Start : overlay « Keep holding to disconnect… », HID neutralisé ;
- appui simple Menu+Start sur écran éteint : réveil sans envoi à l'hôte ;
- navigation Devices/sous-écrans avec D-pad/A/B ;
- B dans l'écran Devices quitte immédiatement, sans confirmation ;
- Écran Devices par défaut sans hôte, sans connexion automatique ;
- Pair a new device reste listé et sélectionnable avec un hôte connecté ;
- retour sur Devices : connexion en cours et appairage arrêtés ;
- A Connect direct sur l'hôte sélectionné, avec le profil mémorisé ;
- popup « Connecting to <hôte> » : blocage des entrées, barre color1 animée,
  texte color4, disparition à la connexion ou après 15 s ;
- X Forget dans Devices : confirmation, oubli de l'hôte, retour à la liste ;
- choix du profil uniquement dans l'écran Pairing, puis réappairage complet ;
- aucune fuite de D-pad/A/B/Menu/Start/Select vers l'hôte dans l'interface ;
- extinction après 10 s, rallumage par appui Menu+Start et nouveau cycle de 10 s ;
- vue controller : titre « Connected », hôte et profil, sans liste d'évènements ;
- choix du profil lors d'un nouvel appairage, redémarrage de session et
  réappairage ;
- vibrations en mode Xbox : rapport 0x03 sur le canal interrupt et SET_REPORT,
  moteur arrêté à la déconnexion et à la sortie ;
- profils PlayStation et Switch : appairage, boutons, sticks centrés, rumble
  et reconnexion ;
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
| Ajouter un profil dans `main.rs` | `match profile` dispersés, session couplée aux protocoles | `profiles/<protocole>.rs` + `AnyCodec` |
| `Box<dyn Trait>` pour les profils | Dispatch dynamique inutile pour un ensemble fermé | `enum` + dispatch statique |
| Profil qui importe `bluez` ou `l2cap` | Protocole couplé au transport, tests impossibles sur l'hôte | `LinkContext`, `Address`, aucun socket dans `profiles` |
| `utils.rs` ou `common.rs` | Dépôt fourre-tout, dépendances illisibles | Module nommé par domaine |
| Analyseur de descripteur recopié par profil | Tests qui divergent et faux positifs | `test_support::descriptor` |
| Logique d'écran dans `profiles` | Profil dépendant de l'interface | `app::view_model` |

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
- profils de manette par hôte (PC, Xbox, puis PlayStation et Switch) ;
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
- **Phase 1b** : profils PlayStation (DualShock 4) et Switch (Pro Controller)
  par hôte.
- **Phase 2** : Brick Pro et Smart Pro.
- **Phase 3** : Smart Pro S `tg5050`.
- **Phase 4** : H700, si nécessaire.
- **Recherche séparée** : USB HID (`f_hid`, FunctionFS ou module noyau).
