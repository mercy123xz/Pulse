//! Translations. `tr("Liked Songs")` returns the text in the chosen language,
//! or the English text when there's no translation yet.

use crate::settings::Language;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_language(l: Language) {
    LANG.store(l as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::ALL[LANG.load(Ordering::Relaxed) as usize]
}

/// The text in the current language.
pub fn tr(en: &str) -> &str {
    let col = match language() {
        Language::English => return en,
        Language::Spanish => 0,
        Language::Portuguese => 1,
        Language::French => 2,
        Language::German => 3,
    };
    table().get(en).map(|row| row[col]).unwrap_or(en)
}

fn table() -> &'static HashMap<&'static str, [&'static str; 4]> {
    static T: OnceLock<HashMap<&'static str, [&'static str; 4]>> = OnceLock::new();
    T.get_or_init(|| STRINGS.iter().map(|(en, rest)| (*en, *rest)).collect())
}

/// English → [Spanish, Portuguese, French, German]
const STRINGS: &[(&str, [&str; 4])] = &[
    // Sidebar and top bar
    ("Browse", ["Explorar", "Explorar", "Parcourir", "Entdecken"]),
    ("Home", ["Inicio", "Início", "Accueil", "Startseite"]),
    ("Charts", ["Listas", "Paradas", "Classements", "Charts"]),
    ("Search", ["Buscar", "Buscar", "Rechercher", "Suchen"]),
    ("Library", ["Biblioteca", "Biblioteca", "Bibliothèque", "Bibliothek"]),
    ("Liked Songs", ["Canciones que te gustan", "Músicas Curtidas", "Titres likés", "Lieblingssongs"]),
    ("Recently Played", ["Escuchado recientemente", "Tocadas recentemente", "Écoutés récemment", "Zuletzt gehört"]),
    ("Queue", ["Cola", "Fila", "File d'attente", "Warteschlange"]),
    ("Local Files", ["Archivos locales", "Arquivos locais", "Fichiers locaux", "Lokale Dateien"]),
    ("Playlists", ["Playlists", "Playlists", "Playlists", "Playlists"]),
    ("Create playlist", ["Crear playlist", "Criar playlist", "Créer une playlist", "Playlist erstellen"]),
    ("Make your first playlist with +", ["Crea tu primera playlist con +", "Crie sua primeira playlist com +", "Créez votre première playlist avec +", "Erstelle deine erste Playlist mit +"]),
    ("Rename", ["Cambiar nombre", "Renomear", "Renommer", "Umbenennen"]),
    ("Delete", ["Eliminar", "Excluir", "Supprimer", "Löschen"]),
    ("Back", ["Atrás", "Voltar", "Retour", "Zurück"]),
    ("Forward", ["Adelante", "Avançar", "Suivant", "Vorwärts"]),
    ("Settings", ["Configuración", "Configurações", "Paramètres", "Einstellungen"]),
    ("Search songs, artists, playlists", ["Busca canciones, artistas, playlists", "Busque músicas, artistas, playlists", "Rechercher des titres, artistes, playlists", "Songs, Künstler, Playlists suchen"]),
    ("Music info by Deezer", ["Información musical de Deezer", "Informações musicais do Deezer", "Infos musicales par Deezer", "Musikinfos von Deezer"]),
    ("Switch to dark mode", ["Cambiar a modo oscuro", "Mudar para modo escuro", "Passer en mode sombre", "Zum dunklen Modus wechseln"]),
    ("Switch to light mode", ["Cambiar a modo claro", "Mudar para modo claro", "Passer en mode clair", "Zum hellen Modus wechseln"]),
    // Song menu
    ("Play", ["Reproducir", "Tocar", "Lecture", "Abspielen"]),
    ("Add to playlist", ["Agregar a playlist", "Adicionar à playlist", "Ajouter à une playlist", "Zur Playlist hinzufügen"]),
    ("New playlist…", ["Nueva playlist…", "Nova playlist…", "Nouvelle playlist…", "Neue Playlist…"]),
    ("Save to Liked Songs", ["Guardar en Canciones que te gustan", "Salvar em Músicas Curtidas", "Ajouter aux Titres likés", "Zu Lieblingssongs hinzufügen"]),
    ("Remove from Liked Songs", ["Quitar de Canciones que te gustan", "Remover de Músicas Curtidas", "Retirer des Titres likés", "Aus Lieblingssongs entfernen"]),
    ("Add to queue", ["Agregar a la cola", "Adicionar à fila", "Ajouter à la file d'attente", "Zur Warteschlange hinzufügen"]),
    ("Play next", ["Reproducir a continuación", "Tocar em seguida", "Lire ensuite", "Als Nächstes abspielen"]),
    ("Go to artist", ["Ir al artista", "Ir para o artista", "Accéder à l'artiste", "Zum Künstler"]),
    ("Go to album", ["Ir al álbum", "Ir para o álbum", "Accéder à l'album", "Zum Album"]),
    ("View credits", ["Ver créditos", "Ver créditos", "Voir les crédits", "Mitwirkende anzeigen"]),
    ("Share", ["Compartir", "Compartilhar", "Partager", "Teilen"]),
    ("Copy YouTube link", ["Copiar enlace de YouTube", "Copiar link do YouTube", "Copier le lien YouTube", "YouTube-Link kopieren"]),
    ("Copy Spotify link", ["Copiar enlace de Spotify", "Copiar link do Spotify", "Copier le lien Spotify", "Spotify-Link kopieren"]),
    ("Copy Deezer link", ["Copiar enlace de Deezer", "Copiar link do Deezer", "Copier le lien Deezer", "Deezer-Link kopieren"]),
    ("Link copied", ["Enlace copiado", "Link copiado", "Lien copié", "Link kopiert"]),
    ("Remove from this playlist", ["Quitar de esta playlist", "Remover desta playlist", "Retirer de cette playlist", "Aus dieser Playlist entfernen"]),
    ("Remove from queue", ["Quitar de la cola", "Remover da fila", "Retirer de la file d'attente", "Aus der Warteschlange entfernen"]),
    ("Download", ["Descargar", "Baixar", "Télécharger", "Herunterladen"]),
    ("Remove download", ["Eliminar descarga", "Remover download", "Supprimer le téléchargement", "Download entfernen"]),
    // Credits
    ("Credits", ["Créditos", "Créditos", "Crédits", "Mitwirkende"]),
    ("Loading credits…", ["Cargando créditos…", "Carregando créditos…", "Chargement des crédits…", "Mitwirkende werden geladen…"]),
    ("Main artist", ["Artista principal", "Artista principal", "Artiste principal", "Hauptkünstler"]),
    ("Featured", ["Invitado", "Participação", "Invité", "Gast"]),
    ("Album", ["Álbum", "Álbum", "Album", "Album"]),
    ("Released", ["Lanzamiento", "Lançamento", "Sortie", "Veröffentlicht"]),
    ("Genre", ["Género", "Gênero", "Genre", "Genre"]),
    ("Label", ["Sello", "Gravadora", "Label", "Label"]),
    ("Tempo", ["Tempo", "Andamento", "Tempo", "Tempo"]),
    ("Close", ["Cerrar", "Fechar", "Fermer", "Schließen"]),
    // Settings
    ("Language", ["Idioma", "Idioma", "Langue", "Sprache"]),
    ("Choose language", ["Elegir idioma", "Escolher idioma", "Choisir la langue", "Sprache wählen"]),
    ("Changes apply right away.", ["Los cambios se aplican al instante.", "As mudanças se aplicam na hora.", "Les modifications s'appliquent immédiatement.", "Änderungen gelten sofort."]),
    ("Autoplay", ["Reproducción automática", "Reprodução automática", "Lecture automatique", "Autoplay"]),
    ("When your queue ends, keep playing similar songs.", ["Cuando termine tu cola, sigue sonando música similar.", "Quando sua fila acabar, continue tocando músicas parecidas.", "Quand votre file d'attente se termine, des titres similaires continuent.", "Wenn deine Warteschlange endet, laufen ähnliche Songs weiter."]),
    ("Audio quality", ["Calidad de audio", "Qualidade de áudio", "Qualité audio", "Audioqualität"]),
    ("Streaming quality", ["Calidad de streaming", "Qualidade do streaming", "Qualité du streaming", "Streamingqualität"]),
    ("Low uses less data. High is the best YouTube offers (usually 128 kbps AAC).", ["Baja usa menos datos. Alta es la mejor que ofrece YouTube (normalmente AAC a 128 kbps).", "Baixa usa menos dados. Alta é a melhor que o YouTube oferece (geralmente AAC a 128 kbps).", "Faible consomme moins de données. Élevée est la meilleure qualité de YouTube (souvent AAC 128 kbps).", "Niedrig spart Daten. Hoch ist das Beste, was YouTube bietet (meist AAC mit 128 kbit/s)."]),
    ("Quality of songs you download for offline listening.", ["Calidad de las canciones que descargas para escuchar sin conexión.", "Qualidade das músicas que você baixa para ouvir offline.", "Qualité des titres téléchargés pour l'écoute hors connexion.", "Qualität der Songs, die du zum Offline-Hören herunterlädst."]),
    ("Automatic", ["Automática", "Automática", "Automatique", "Automatisch"]),
    ("Low", ["Baja", "Baixa", "Faible", "Niedrig"]),
    ("Medium", ["Media", "Média", "Moyenne", "Mittel"]),
    ("High", ["Alta", "Alta", "Élevée", "Hoch"]),
    ("Your Library", ["Tu biblioteca", "Sua biblioteca", "Votre bibliothèque", "Deine Bibliothek"]),
    ("Show Local Files", ["Mostrar archivos locales", "Mostrar arquivos locais", "Afficher les fichiers locaux", "Lokale Dateien anzeigen"]),
    ("Songs from your Music folder:", ["Canciones de tu carpeta Música:", "Músicas da sua pasta Músicas:", "Titres de votre dossier Musique :", "Songs aus deinem Musikordner:"]),
    ("Playback", ["Reproducción", "Reprodução", "Lecture", "Wiedergabe"]),
    ("Crossfade songs", ["Fundido entre canciones", "Crossfade entre músicas", "Fondu enchaîné", "Songs überblenden"]),
    ("Fade each song into the next.", ["Funde cada canción con la siguiente.", "Funde cada música na próxima.", "Enchaîne chaque titre avec le suivant en fondu.", "Blendet jeden Song in den nächsten über."]),
    ("Gapless playback", ["Reproducción sin pausas", "Reprodução sem intervalos", "Lecture sans blanc", "Lückenlose Wiedergabe"]),
    ("Start the next song with no silence in between.", ["Empieza la siguiente canción sin silencio entre medias.", "Começa a próxima música sem silêncio entre elas.", "Lance le titre suivant sans silence.", "Startet den nächsten Song ohne Pause."]),
    ("Automix", ["Automix", "Automix", "Automix", "Automix"]),
    ("Allow seamless transitions between songs: skips silent endings and blends into the next song.", ["Transiciones fluidas entre canciones: salta los finales en silencio y mezcla con la siguiente.", "Transições suaves entre músicas: pula finais silenciosos e mistura com a próxima.", "Transitions fluides entre les titres : saute les fins silencieuses et enchaîne avec le suivant.", "Nahtlose Übergänge: überspringt stille Enden und mischt in den nächsten Song."]),
    ("Mono audio", ["Audio mono", "Áudio mono", "Audio mono", "Mono-Audio"]),
    ("Plays the same sound from left and right speakers.", ["Reproduce el mismo sonido por los altavoces izquierdo y derecho.", "Toca o mesmo som nos alto-falantes esquerdo e direito.", "Diffuse le même son à gauche et à droite.", "Spielt links und rechts denselben Ton."]),
    ("Equalizer", ["Ecualizador", "Equalizador", "Égaliseur", "Equalizer"]),
    ("Shape the sound with presets or by dragging the points.", ["Ajusta el sonido con preajustes o arrastrando los puntos.", "Ajuste o som com predefinições ou arrastando os pontos.", "Modelez le son avec des préréglages ou en faisant glisser les points.", "Forme den Klang mit Voreinstellungen oder durch Ziehen der Punkte."]),
    ("Presets", ["Preajustes", "Predefinições", "Préréglages", "Voreinstellungen"]),
    ("Manual", ["Manual", "Manual", "Manuel", "Manuell"]),
    ("Flat", ["Plano", "Plano", "Neutre", "Flach"]),
    ("Bass Booster", ["Refuerzo de graves", "Reforço de graves", "Renforcement des basses", "Bass-Booster"]),
    ("Bass Reducer", ["Reductor de graves", "Redutor de graves", "Réduction des basses", "Bass-Reduzierer"]),
    ("Treble Booster", ["Refuerzo de agudos", "Reforço de agudos", "Renforcement des aigus", "Höhen-Booster"]),
    ("Vocal Booster", ["Refuerzo de voces", "Reforço vocal", "Renforcement des voix", "Gesangs-Booster"]),
    ("Loudness", ["Sonoridad", "Loudness", "Loudness", "Loudness"]),
    ("Acoustic", ["Acústico", "Acústico", "Acoustique", "Akustik"]),
    ("Electronic", ["Electrónica", "Eletrônica", "Électronique", "Elektronisch"]),
    ("Save preset", ["Guardar preajuste", "Salvar predefinição", "Enregistrer le préréglage", "Voreinstellung speichern"]),
    ("Delete preset", ["Eliminar preajuste", "Excluir predefinição", "Supprimer le préréglage", "Voreinstellung löschen"]),
    ("My preset", ["Mi preajuste", "Minha predefinição", "Mon préréglage", "Meine Voreinstellung"]),
    ("Reset", ["Restablecer", "Redefinir", "Réinitialiser", "Zurücksetzen"]),
    ("band", ["banda", "faixa", "bande", "Band"]),
    ("Storage", ["Almacenamiento", "Armazenamento", "Stockage", "Speicher"]),
    ("Cache", ["Caché", "Cache", "Cache", "Cache"]),
    ("Temporary files Pulse can download again:", ["Archivos temporales que Pulse puede volver a descargar:", "Arquivos temporários que o Pulse pode baixar de novo:", "Fichiers temporaires que Pulse peut retélécharger :", "Temporäre Dateien, die Pulse neu laden kann:"]),
    ("Clear cache", ["Borrar caché", "Limpar cache", "Vider le cache", "Cache leeren"]),
    ("Cache cleared", ["Caché borrada", "Cache limpo", "Cache vidé", "Cache geleert"]),
    ("Offline storage location", ["Ubicación de almacenamiento sin conexión", "Local de armazenamento offline", "Emplacement du stockage hors connexion", "Speicherort für Offline-Songs"]),
    ("Change location", ["Cambiar ubicación", "Mudar local", "Modifier l'emplacement", "Ort ändern"]),
    ("Compatibility", ["Compatibilidad", "Compatibilidade", "Compatibilité", "Kompatibilität"]),
    ("High contrast mode", ["Modo de alto contraste", "Modo de alto contraste", "Mode contraste élevé", "Hoher Kontrast"]),
    ("Stronger colors and borders, easier to read.", ["Colores y bordes más marcados, más fácil de leer.", "Cores e bordas mais fortes, mais fácil de ler.", "Couleurs et bordures plus marquées, plus lisible.", "Kräftigere Farben und Ränder, besser lesbar."]),
    ("Enable hardware acceleration", ["Activar aceleración por hardware", "Ativar aceleração de hardware", "Activer l'accélération matérielle", "Hardwarebeschleunigung aktivieren"]),
    ("Uses your graphics card to draw Pulse. Turn off if the window flickers or stays blank. Restart Pulse to apply.", ["Usa tu tarjeta gráfica para dibujar Pulse. Desactívala si la ventana parpadea o se queda en blanco. Reinicia Pulse para aplicar.", "Usa sua placa de vídeo para desenhar o Pulse. Desative se a janela piscar ou ficar em branco. Reinicie o Pulse para aplicar.", "Utilise votre carte graphique pour afficher Pulse. Désactivez-la si la fenêtre clignote ou reste vide. Redémarrez Pulse pour appliquer.", "Nutzt deine Grafikkarte zum Zeichnen. Schalte sie aus, wenn das Fenster flackert oder leer bleibt. Pulse neu starten, um es anzuwenden."]),
    ("Restart Pulse to apply this change.", ["Reinicia Pulse para aplicar este cambio.", "Reinicie o Pulse para aplicar esta mudança.", "Redémarrez Pulse pour appliquer ce changement.", "Starte Pulse neu, um die Änderung anzuwenden."]),
    // Player and pages
    ("Now playing", ["Reproduciendo", "Tocando agora", "En cours de lecture", "Läuft gerade"]),
    ("Next up", ["A continuación", "A seguir", "À suivre", "Als Nächstes"]),
    ("Clear", ["Borrar", "Limpar", "Effacer", "Leeren"]),
    ("Title", ["Título", "Título", "Titre", "Titel"]),
    ("Date added", ["Fecha en que se agregó", "Adicionada em", "Date d'ajout", "Hinzugefügt am"]),
    ("Recently added", ["Agregadas recientemente", "Adicionadas recentemente", "Ajoutés récemment", "Zuletzt hinzugefügt"]),
    ("Artist", ["Artista", "Artista", "Artiste", "Künstler"]),
    ("Duration", ["Duración", "Duração", "Durée", "Dauer"]),
    ("Custom order", ["Orden personalizado", "Ordem personalizada", "Ordre personnalisé", "Eigene Reihenfolge"]),
    ("Search in playlist", ["Buscar en la playlist", "Buscar na playlist", "Rechercher dans la playlist", "In Playlist suchen"]),
    ("All", ["Todo", "Tudo", "Tout", "Alle"]),
    ("Shuffle", ["Aleatorio", "Aleatório", "Aléatoire", "Zufällig"]),
    ("Change cover", ["Cambiar portada", "Mudar capa", "Changer la pochette", "Cover ändern"]),
    ("Remove cover", ["Quitar portada", "Remover capa", "Retirer la pochette", "Cover entfernen"]),
    ("Downloaded", ["Descargada", "Baixada", "Téléchargé", "Heruntergeladen"]),
    ("Downloading…", ["Descargando…", "Baixando…", "Téléchargement…", "Wird heruntergeladen…"]),
    ("Today", ["Hoy", "Hoje", "Aujourd'hui", "Heute"]),
    ("Yesterday", ["Ayer", "Ontem", "Hier", "Gestern"]),
    ("Sort by", ["Ordenar por", "Ordenar por", "Trier par", "Sortieren nach"]),
    ("No songs match.", ["Ninguna canción coincide.", "Nenhuma música corresponde.", "Aucun titre ne correspond.", "Keine passenden Songs."]),
    ("Delete playlist", ["Eliminar playlist", "Excluir playlist", "Supprimer la playlist", "Playlist löschen"]),
    ("Playlist", ["Playlist", "Playlist", "Playlist", "Playlist"]),
    ("It's quiet in here", ["Esto está muy tranquilo", "Está tudo quieto por aqui", "C'est bien calme ici", "Hier ist es still"]),
    ("Themes", ["Temas", "Temas", "Thèmes", "Designs"]),
    ("Theme", ["Tema", "Tema", "Thème", "Design"]),
    ("Color scheme", ["Esquema de colores", "Esquema de cores", "Jeu de couleurs", "Farbschema"]),
    ("Open themes folder", ["Abrir carpeta de temas", "Abrir pasta de temas", "Ouvrir le dossier des thèmes", "Design-Ordner öffnen"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_falls_back_to_english() {
        set_language(Language::Spanish);
        assert_eq!(tr("Liked Songs"), "Canciones que te gustan");
        assert_eq!(tr("Not in the table"), "Not in the table");
        set_language(Language::German);
        assert_eq!(tr("Settings"), "Einstellungen");
        set_language(Language::English);
        assert_eq!(tr("Settings"), "Settings");
    }

    #[test]
    fn no_duplicate_or_empty_entries() {
        let mut seen = std::collections::HashSet::new();
        for (en, rest) in STRINGS {
            assert!(seen.insert(*en), "duplicate: {en}");
            assert!(rest.iter().all(|s| !s.is_empty()), "empty translation for {en}");
        }
    }
}
