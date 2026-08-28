// src/runtime/compatibility/search-labels.ts
var SEARCH_LABELS = new Set([
  "Search",
  "搜索",
  "搜尋",
  "搜寻",
  "検索",
  "검색",
  "Rechercher",
  "Suche",
  "Buscar",
  "Cerca",
  "Pesquisar",
  "Procurar",
  "Поиск",
  "Пошук",
  "Szukaj",
  "Hledat",
  "Hľadať",
  "Keresés",
  "Căutare",
  "Ara",
  "Søk",
  "Sök",
  "Søg",
  "Zoeken",
  "Hae",
  "Αναζήτηση",
  "חיפוש",
  "بحث",
  "खोजें",
  "खोज",
  "ค้นหา",
  "Tìm kiếm",
  "Cari",
  "Pencarian"
]);
function isSearchLabel(label) {
  const value = label?.trim() ?? "";
  if (!value)
    return false;
  if (SEARCH_LABELS.has(value))
    return true;
  const lower = value.toLowerCase();
  return lower === "search" || lower.startsWith("search ");
}

// src/runtime/incodex-ui-probe.ts
function deriveUiProbe(input) {
  const button = input.buttonPresent ? "present" : "missing";
  const tooltip = input.tooltipPresent ? "present" : "missing";
  let banner;
  if (!input.incognito) {
    banner = "not-applicable";
  } else if (input.bannerPresent) {
    banner = "present";
  } else if (input.bannerDismissed) {
    banner = "dismissed";
  } else {
    banner = "missing";
  }
  return {
    button,
    banner,
    tooltip,
    accepted: button === "present" && tooltip === "present" && banner !== "missing"
  };
}

// src/runtime/incognito-copy-data.ts
var COPY = {
  am: {
    open: "የግል መስኮት ክፈት",
    exit: "የግል መስኮትን ውጣ",
    title: "የግል መስኮት",
    body: "መለያ እና ቅንብሮች እንደተለመደው ናቸው፣ ቀደም ያሉ ውይይቶች አይመጡም። ይህን መስኮት ሲዘጉ ይህ ውይይት አይቀመጥም።",
    dismiss: "የግል መስኮት ማስታወቂያን ዝጋ",
    errorTitle: "የግል መስኮት መክፈት አልተቻለም",
    errorBody: "እንደገና ይሞክሩ። ካልሆነ Codex ን ዝጉና እንደገና ይክፈቱ።",
    errorRetry: "እንደገና ሞክር",
    errorClose: "ዝጋ"
  },
  ar: {
    open: "فتح نافذة التصفح المتخفي",
    exit: "الخروج من نافذة التصفح المتخفي",
    title: "نافذة التصفح المتخفي",
    body: "الحساب والإعدادات كما هي عادةً، دون المحادثات السابقة. أغلق هذه النافذة ولن يُحفظ سجل هذه المحادثة.",
    dismiss: "إغلاق بانر التصفح المتخفي",
    errorTitle: "تعذّر فتح نافذة التصفح المتخفي",
    errorBody: "حاول مرة أخرى. إذا استمر الفشل، أغلق Codex ثم افتحه من جديد.",
    errorRetry: "إعادة المحاولة",
    errorClose: "إغلاق"
  },
  "bg-BG": {
    open: "Отваряне на прозорец инкогнито",
    exit: "Изход от прозорец инкогнито",
    title: "Прозорец инкогнито",
    body: "Акаунтът и настройките са както обикновено, без предишни разговори. Затворете този прозорец и този чат няма да остави запис.",
    dismiss: "Затваряне на банера за инкогнито",
    errorTitle: "Прозорецът инкогнито не можа да се отвори",
    errorBody: "Опитайте отново. Ако пак не стане, затворете Codex и го отворете отново.",
    errorRetry: "Опитай отново",
    errorClose: "Затвори"
  },
  "bn-BD": {
    open: "ইনকগনিটো উইন্ডো খুলুন",
    exit: "ইনকগনিটো উইন্ডো থেকে বেরোন",
    title: "ইনকগনিটো উইন্ডো",
    body: "অ্যাকাউন্ট ও সেটিংস আগের মতোই, আগের আলোচনা আসবে না। এই উইন্ডো বন্ধ করলে এবারের চ্যাটের রেকর্ড থাকবে না।",
    dismiss: "ইনকগনিটো ব্যানার বন্ধ করুন",
    errorTitle: "ইনকগনিটো উইন্ডো খোলা যায়নি",
    errorBody: "আবার চেষ্টা করুন। না হলে Codex বন্ধ করে আবার খুলুন।",
    errorRetry: "আবার চেষ্টা করুন",
    errorClose: "বন্ধ করুন"
  },
  "bs-BA": {
    open: "Otvori prozor incognito",
    exit: "Izađi iz prozora incognito",
    title: "Prozor incognito",
    body: "Račun i postavke su kao i obično, bez prethodnih razgovora. Zatvorite ovaj prozor i ovaj chat neće ostaviti zapis.",
    dismiss: "Zatvori incognito banner",
    errorTitle: "Nije moguće otvoriti prozor incognito",
    errorBody: "Pokušajte ponovo. Ako i dalje ne radi, zatvorite Codex i otvorite ga ponovo.",
    errorRetry: "Pokušaj ponovo",
    errorClose: "Zatvori"
  },
  "ca-ES": {
    open: "Obre una finestra d'incògnit",
    exit: "Surt de la finestra d'incògnit",
    title: "Finestra d'incògnit",
    body: "El compte i la configuració són els de sempre, sense converses anteriors. Si tanques aquesta finestra, aquest xat no deixarà cap registre.",
    dismiss: "Tanca el bàner d'incògnit",
    errorTitle: "No s'ha pogut obrir la finestra d'incògnit",
    errorBody: "Torna-ho a provar. Si continua fallant, surt de Codex i torna'l a obrir.",
    errorRetry: "Torna-ho a provar",
    errorClose: "Tanca"
  },
  "cs-CZ": {
    open: "Otevřít anonymní okno",
    exit: "Ukončit anonymní okno",
    title: "Anonymní okno",
    body: "Účet a nastavení jsou jako obvykle, bez předchozích konverzací. Po zavření tohoto okna se tento chat nezaznamená.",
    dismiss: "Zavřít banner anonymního okna",
    errorTitle: "Anonymní okno se nepodařilo otevřít",
    errorBody: "Zkuste to znovu. Pokud to stále nejde, ukončete Codex a otevřete ho znovu.",
    errorRetry: "Zkusit znovu",
    errorClose: "Zavřít"
  },
  "da-DK": {
    open: "Åbn inkognitovindue",
    exit: "Afslut inkognitovindue",
    title: "Inkognitovindue",
    body: "Konto og indstillinger er som sædvanlig, uden tidligere samtaler. Lukker du dette vindue, efterlader denne chat ingen optegnelse.",
    dismiss: "Luk inkognitobanner",
    errorTitle: "Inkognitovinduet kunne ikke åbnes",
    errorBody: "Prøv igen. Virker det stadig ikke, så afslut Codex og åbn det igen.",
    errorRetry: "Prøv igen",
    errorClose: "Luk"
  },
  "de-DE": {
    open: "Inkognito-Fenster öffnen",
    exit: "Inkognito-Fenster beenden",
    title: "Inkognito-Fenster",
    body: "Konto und Einstellungen sind wie sonst, ohne frühere Unterhaltungen. Wenn du dieses Fenster schließt, bleibt von diesem Chat kein Verlauf.",
    dismiss: "Inkognito-Banner schließen",
    errorTitle: "Inkognito-Fenster konnte nicht geöffnet werden",
    errorBody: "Bitte erneut versuchen. Wenn es weiterhin fehlschlägt, Codex beenden und neu öffnen.",
    errorRetry: "Erneut versuchen",
    errorClose: "Schließen"
  },
  "el-GR": {
    open: "Άνοιγμα παραθύρου ανώνυμης περιήγησης",
    exit: "Έξοδος από το παράθυρο ανώνυμης περιήγησης",
    title: "Παράθυρο ανώνυμης περιήγησης",
    body: "Ο λογαριασμός και οι ρυθμίσεις είναι όπως συνήθως, χωρίς προηγούμενες συνομιλίες. Κλείστε αυτό το παράθυρο και αυτή η συνομιλία δεν θα αφήσει εγγραφή.",
    dismiss: "Κλείσιμο του banner ανώνυμης περιήγησης",
    errorTitle: "Δεν ήταν δυνατό το άνοιγμα του παραθύρου ανώνυμης περιήγησης",
    errorBody: "Δοκιμάστε ξανά. Αν συνεχίσει να αποτυγχάνει, κλείστε το Codex και ανοίξτε το ξανά.",
    errorRetry: "Δοκιμή ξανά",
    errorClose: "Κλείσιμο"
  },
  "es-419": {
    open: "Abrir ventana de incógnito",
    exit: "Salir de la ventana de incógnito",
    title: "Ventana de incógnito",
    body: "La cuenta y la configuración son las de siempre, sin conversaciones anteriores. Si cierras esta ventana, este chat no dejará registro.",
    dismiss: "Cerrar el banner de incógnito",
    errorTitle: "No se pudo abrir la ventana de incógnito",
    errorBody: "Inténtalo de nuevo. Si sigue fallando, cierra Codex y ábrelo otra vez.",
    errorRetry: "Reintentar",
    errorClose: "Cerrar"
  },
  "es-ES": {
    open: "Abrir ventana de incógnito",
    exit: "Salir de la ventana de incógnito",
    title: "Ventana de incógnito",
    body: "La cuenta y la configuración son las de siempre, sin conversaciones anteriores. Si cierras esta ventana, este chat no dejará registro.",
    dismiss: "Cerrar el banner de incógnito",
    errorTitle: "No se ha podido abrir la ventana de incógnito",
    errorBody: "Inténtalo de nuevo. Si sigue fallando, cierra Codex y ábrelo otra vez.",
    errorRetry: "Reintentar",
    errorClose: "Cerrar"
  },
  "et-EE": {
    open: "Ava inkognitoaken",
    exit: "Välju inkognitoaknast",
    title: "Inkognitoaken",
    body: "Konto ja seaded on tavapärased, varasemaid vestlusi kaasa ei tooda. Kui sulged selle akna, ei jää sellest vestlusest jälge.",
    dismiss: "Sulge inkognitobänner",
    errorTitle: "Inkognitoakent ei saanud avada",
    errorBody: "Proovi uuesti. Kui ikka ei õnnestu, sulge Codex ja ava see uuesti.",
    errorRetry: "Proovi uuesti",
    errorClose: "Sulge"
  },
  fa: {
    open: "باز کردن پنجره ناشناس",
    exit: "خروج از پنجره ناشناس",
    title: "پنجره ناشناس",
    body: "حساب و تنظیمات مثل همیشه است و گفتگوهای قبلی وارد نمی‌شود. با بستن این پنجره، این گفتگو ثبت نمی‌ماند.",
    dismiss: "بستن بنر پنجره ناشناس",
    errorTitle: "پنجره ناشناس باز نشد",
    errorBody: "دوباره امتحان کنید. اگر باز هم نشد، Codex را ببندید و دوباره باز کنید.",
    errorRetry: "تلاش دوباره",
    errorClose: "بستن"
  },
  "fi-FI": {
    open: "Avaa incognito-ikkuna",
    exit: "Poistu incognito-ikkunasta",
    title: "Incognito-ikkuna",
    body: "Tili ja asetukset ovat ennallaan, aiempia keskusteluja ei tuoda. Jos suljet tämän ikkunan, tästä chatista ei jää merkintää.",
    dismiss: "Sulje incognito-banneri",
    errorTitle: "Incognito-ikkunaa ei voitu avata",
    errorBody: "Yritä uudelleen. Jos se ei vieläkään onnistu, sulje Codex ja avaa se uudestaan.",
    errorRetry: "Yritä uudelleen",
    errorClose: "Sulje"
  },
  "fr-CA": {
    open: "Ouvrir une fenêtre de navigation privée",
    exit: "Quitter la fenêtre de navigation privée",
    title: "Fenêtre de navigation privée",
    body: "Le compte et les réglages sont les mêmes qu’à l’habitude, sans les conversations précédentes. Fermez cette fenêtre et ce clavardage ne laissera aucune trace.",
    dismiss: "Fermer la bannière de navigation privée",
    errorTitle: "Impossible d’ouvrir la fenêtre de navigation privée",
    errorBody: "Réessayez. Si ça échoue encore, quittez Codex puis rouvrez-le.",
    errorRetry: "Réessayer",
    errorClose: "Fermer"
  },
  "fr-FR": {
    open: "Ouvrir une fenêtre de navigation privée",
    exit: "Quitter la fenêtre de navigation privée",
    title: "Fenêtre de navigation privée",
    body: "Le compte et les paramètres sont les mêmes que d’habitude, sans les conversations précédentes. Fermez cette fenêtre et cette discussion ne laissera aucune trace.",
    dismiss: "Fermer la bannière de navigation privée",
    errorTitle: "Impossible d’ouvrir la fenêtre de navigation privée",
    errorBody: "Réessayez. Si cela échoue encore, quittez Codex puis rouvrez-le.",
    errorRetry: "Réessayer",
    errorClose: "Fermer"
  },
  "gu-IN": {
    open: "ઇનકોગ્નિટો વિન્ડો ખોલો",
    exit: "ઇનકોગ્નિટો વિન્ડોમાંથી બહાર નીકળો",
    title: "ઇનકોગ્નિટો વિન્ડો",
    body: "એકાઉન્ટ અને સેટિંગ્સ હંમેશની જેમ છે, અગાઉની વાતચીતો આવશે નહીં. આ વિન્ડો બંધ કરશો તો આ ચેટનો રેકોર્ડ રહેશે નહીં.",
    dismiss: "ઇનકોગ્નિટો બેનર બંધ કરો",
    errorTitle: "ઇનકોગ્નિટો વિન્ડો ખોલી શકાઈ નહીં",
    errorBody: "ફરી પ્રયાસ કરો. હજુ ન થાય તો Codex બંધ કરીને ફરી ખોલો.",
    errorRetry: "ફરી પ્રયાસ કરો",
    errorClose: "બંધ કરો"
  },
  "hi-IN": {
    open: "गुप्त विंडो खोलें",
    exit: "गुप्त विंडो से बाहर निकलें",
    title: "गुप्त विंडो",
    body: "खाता और सेटिंग्स वैसे ही हैं, पिछली बातचीत नहीं आएगी। यह विंडो बंद करने पर इस चैट का रिकॉर्ड नहीं रहेगा।",
    dismiss: "गुप्त विंडो बैनर बंद करें",
    errorTitle: "गुप्त विंडो नहीं खुल सकी",
    errorBody: "फिर कोशिश करें। अगर फिर भी न खुले, तो Codex बंद करके फिर खोलें।",
    errorRetry: "फिर कोशिश करें",
    errorClose: "बंद करें"
  },
  "hr-HR": {
    open: "Otvori prozor incognito",
    exit: "Izađi iz prozora incognito",
    title: "Prozor incognito",
    body: "Račun i postavke su kao i obično, bez prethodnih razgovora. Zatvorite ovaj prozor i ovaj chat neće ostaviti zapis.",
    dismiss: "Zatvori incognito banner",
    errorTitle: "Nije moguće otvoriti prozor incognito",
    errorBody: "Pokušajte ponovo. Ako i dalje ne radi, zatvorite Codex i otvorite ga ponovo.",
    errorRetry: "Pokušaj ponovo",
    errorClose: "Zatvori"
  },
  "hu-HU": {
    open: "Inkognitóablak megnyitása",
    exit: "Kilépés az inkognitóablakból",
    title: "Inkognitóablak",
    body: "A fiók és a beállítások a szokásosak, korábbi beszélgetések nélkül. Ha bezárod ezt az ablakot, ebből a csevegésből nem marad nyom.",
    dismiss: "Inkognitó banner bezárása",
    errorTitle: "Nem sikerült megnyitni az inkognitóablakot",
    errorBody: "Próbáld újra. Ha továbbra sem megy, zárd be a Codexet, majd nyisd meg újra.",
    errorRetry: "Újra",
    errorClose: "Bezárás"
  },
  "hy-AM": {
    open: "Բացել գաղտնի պատուհանը",
    exit: "Դուրս գալ գաղտնի պատուհանից",
    title: "Գաղտնի պատուհան",
    body: "Հաշիվն ու կարգավորումները սովորականի պես են, նախորդ զրույցները չեն բերվի։ Այս պատուհանը փակելիս այս զրույցը գրառում չի թողնի։",
    dismiss: "Փակել գաղտնի պատուհանի դրոշակը",
    errorTitle: "Չհաջողվեց բացել գաղտնի պատուհանը",
    errorBody: "Կրկին փորձեք։ Եթե դեռ չի ստացվում, փակեք Codex-ը և նորից բացեք։",
    errorRetry: "Կրկին փորձել",
    errorClose: "Փակել"
  },
  "id-ID": {
    open: "Buka jendela penyamaran",
    exit: "Keluar dari jendela penyamaran",
    title: "Jendela penyamaran",
    body: "Akun dan pengaturan sama seperti biasa, tanpa percakapan sebelumnya. Tutup jendela ini dan obrolan ini tidak akan meninggalkan catatan.",
    dismiss: "Tutup banner jendela penyamaran",
    errorTitle: "Tidak dapat membuka jendela penyamaran",
    errorBody: "Coba lagi. Jika masih gagal, keluar dari Codex lalu buka lagi.",
    errorRetry: "Coba lagi",
    errorClose: "Tutup"
  },
  "is-IS": {
    open: "Opna huliðsglugga",
    exit: "Hætta í huliðsglugga",
    title: "Huliðsgluggi",
    body: "Aðgangur og stillingar eru eins og venjulega, án fyrri spjalla. Ef þú lokar þessum glugga skilur þetta spjall ekkert eftir sig.",
    dismiss: "Loka huliðsbanneri",
    errorTitle: "Gat ekki opnað huliðsglugga",
    errorBody: "Reyndu aftur. Ef það mistakast enn, lokaðu Codex og opnaðu aftur.",
    errorRetry: "Reyna aftur",
    errorClose: "Loka"
  },
  "it-IT": {
    open: "Apri finestra in incognito",
    exit: "Esci dalla finestra in incognito",
    title: "Finestra in incognito",
    body: "Account e impostazioni sono quelli di sempre, senza le chat precedenti. Chiudi questa finestra e questa conversazione non lascerà traccia.",
    dismiss: "Chiudi il banner in incognito",
    errorTitle: "Impossibile aprire la finestra in incognito",
    errorBody: "Riprova. Se continua a non funzionare, chiudi Codex e aprilo di nuovo.",
    errorRetry: "Riprova",
    errorClose: "Chiudi"
  },
  "ja-JP": {
    open: "シークレットウィンドウを開く",
    exit: "シークレットウィンドウを終了",
    title: "シークレットウィンドウ",
    body: "アカウントと設定は普段と同じで、以前の会話は引き継ぎません。このウィンドウを閉じると、今回のチャットは残りません。",
    dismiss: "シークレットウィンドウのバナーを閉じる",
    errorTitle: "シークレットウィンドウを開けませんでした",
    errorBody: "もう一度お試しください。まだ開かない場合は、Codex を終了してから開き直してください。",
    errorRetry: "再試行",
    errorClose: "閉じる"
  },
  "ka-GE": {
    open: "ინკოგნიტო ფანჯრის გახსნა",
    exit: "ინკოგნიტო ფანჯრიდან გასვლა",
    title: "ინკოგნიტო ფანჯარა",
    body: "ანგარიში და პარამეტრები ჩვეულებრივია, წინა საუბრები არ შემოვა. ამ ფანჯრის დახურვის შემდეგ ეს ჩატი ჩანაწერს არ დატოვებს.",
    dismiss: "ინკოგნიტო ბანერის დახურვა",
    errorTitle: "ინკოგნიტო ფანჯარა ვერ გაიხსნა",
    errorBody: "სცადეთ თავიდან. თუ ისევ ვერ გაიხსნა, დახურეთ Codex და გახსენით ხელახლა.",
    errorRetry: "თავიდან ცდა",
    errorClose: "დახურვა"
  },
  kk: {
    open: "Инкогнито терезесін ашу",
    exit: "Инкогнито терезесінен шығу",
    title: "Инкогнито терезесі",
    body: "Тіркелгі мен баптаулар әдеттегідей, бұрынғы әңгімелер кірмейді. Бұл терезені жапсаңыз, осы чат жазба қалдырмайды.",
    dismiss: "Инкогнито баннерін жабу",
    errorTitle: "Инкогнито терезесі ашылмады",
    errorBody: "Қайта көріңіз. Әлі де болмаса, Codex-ті жауып, қайта ашыңыз.",
    errorRetry: "Қайталау",
    errorClose: "Жабу"
  },
  "kn-IN": {
    open: "ಅಜ್ಞಾತ ವಿಂಡೋ ತೆರೆಯಿರಿ",
    exit: "ಅಜ್ಞಾತ ವಿಂಡೋದಿಂದ ನಿರ್ಗಮಿಸಿ",
    title: "ಅಜ್ಞಾತ ವಿಂಡೋ",
    body: "ಖಾತೆ ಮತ್ತು ಸೆಟ್ಟಿಂಗ್‌ಗಳು ಎಂದಿನಂತೆ, ಹಿಂದಿನ ಸಂಭಾಷಣೆಗಳು ಬರುವುದಿಲ್ಲ. ಈ ವಿಂಡೋ ಮುಚ್ಚಿದರೆ ಈ ಚಾಟ್‌ನ ದಾಖಲೆ ಉಳಿಯುವುದಿಲ್ಲ.",
    dismiss: "ಅಜ್ಞಾತ ಬ್ಯಾನರ್ ಮುಚ್ಚಿ",
    errorTitle: "ಅಜ್ಞಾತ ವಿಂಡೋ ತೆರೆಯಲಾಗಲಿಲ್ಲ",
    errorBody: "ಮತ್ತೆ ಪ್ರಯತ್ನಿಸಿ. ಇನ್ನೂ ಆಗದಿದ್ದರೆ Codex ಮುಚ್ಚಿ ಮತ್ತೆ ತೆರೆಯಿರಿ.",
    errorRetry: "ಮತ್ತೆ ಪ್ರಯತ್ನಿಸಿ",
    errorClose: "ಮುಚ್ಚಿ"
  },
  "ko-KR": {
    open: "시크릿 창 열기",
    exit: "시크릿 창 나가기",
    title: "시크릿 창",
    body: "계정과 설정은 평소와 같고, 이전 대화는 가져오지 않습니다. 창을 닫으면 이번 채팅은 남지 않습니다.",
    dismiss: "시크릿 창 배너 닫기",
    errorTitle: "시크릿 창을 열 수 없습니다",
    errorBody: "다시 시도하세요. 그래도 안 되면 Codex를 종료한 뒤 다시 여세요.",
    errorRetry: "다시 시도",
    errorClose: "닫기"
  },
  lt: {
    open: "Atidaryti inkognito langą",
    exit: "Išeiti iš inkognito lango",
    title: "Inkognito langas",
    body: "Paskyra ir nustatymai tokie patys kaip įprastai, ankstesni pokalbiai neperkeliami. Užvėrus šį langą šis pokalbis nepaliks įrašo.",
    dismiss: "Uždaryti inkognito juostą",
    errorTitle: "Nepavyko atidaryti inkognito lango",
    errorBody: "Bandykite dar kartą. Jei vis tiek nepavyksta, uždarykite Codex ir atidarykite iš naujo.",
    errorRetry: "Bandyti dar kartą",
    errorClose: "Uždaryti"
  },
  "lv-LV": {
    open: "Atvērt inkognito logu",
    exit: "Iziet no inkognito loga",
    title: "Inkognito logs",
    body: "Konts un iestatījumi ir kā parasti, bez iepriekšējām sarunām. Aizverot šo logu, šī tērzēšana neatstās ierakstu.",
    dismiss: "Aizvērt inkognito baneri",
    errorTitle: "Neizdevās atvērt inkognito logu",
    errorBody: "Mēģiniet vēlreiz. Ja joprojām neizdodas, aizveriet Codex un atveriet to no jauna.",
    errorRetry: "Mēģināt vēlreiz",
    errorClose: "Aizvērt"
  },
  "mk-MK": {
    open: "Отвори прозорец инкогнито",
    exit: "Излези од прозорецот инкогнито",
    title: "Прозорец инкогнито",
    body: "Сметката и поставките се како и обично, без претходни разговори. Ако го затворите овој прозорец, овој разговор нема да остави запис.",
    dismiss: "Затвори го банерот за инкогнито",
    errorTitle: "Не можеше да се отвори прозорецот инкогнито",
    errorBody: "Обидете се повторно. Ако пак не успее, затворете го Codex и отворете го повторно.",
    errorRetry: "Обиди се повторно",
    errorClose: "Затвори"
  },
  ml: {
    open: "ഇൻകോഗ്നിറ്റോ വിൻഡോ തുറക്കുക",
    exit: "ഇൻകോഗ്നിറ്റോ വിൻഡോയിൽ നിന്ന് പുറത്തുകടക്കുക",
    title: "ഇൻകോഗ്നിറ്റോ വിൻഡോ",
    body: "അക്കൗണ്ടും ക്രമീകരണങ്ങളും പതിവുപോലെയാണ്, മുമ്പത്തെ സംഭാഷണങ്ങൾ വരില്ല. ഈ വിൻഡോ അടച്ചാൽ ഈ ചാറ്റിന്റെ രേഖ നിലനിൽക്കില്ല.",
    dismiss: "ഇൻകോഗ്നിറ്റോ ബാനർ അടയ്ക്കുക",
    errorTitle: "ഇൻകോഗ്നിറ്റോ വിൻഡോ തുറക്കാനായില്ല",
    errorBody: "വീണ്ടും ശ്രമിക്കുക. ഇനിയും ആയില്ലെങ്കിൽ Codex അടച്ച് വീണ്ടും തുറക്കുക.",
    errorRetry: "വീണ്ടും ശ്രമിക്കുക",
    errorClose: "അടയ്ക്കുക"
  },
  mn: {
    open: "Нууц цонх нээх",
    exit: "Нууц цонхноос гарах",
    title: "Нууц цонх",
    body: "Бүртгэл, тохиргоо хэвийнхээ л адил, өмнөх яриа орж ирэхгүй. Энэ цонхыг хаавал энэ чат үлдэхгүй.",
    dismiss: "Нууц цонхны баннерыг хаах",
    errorTitle: "Нууц цонх нээгдсэнгүй",
    errorBody: "Дахин оролдоно уу. Хэрэв дахин бүтэхгүй бол Codex-ийг хаагаад дахин нээнэ үү.",
    errorRetry: "Дахин оролдох",
    errorClose: "Хаах"
  },
  "mr-IN": {
    open: "गुप्त विंडो उघडा",
    exit: "गुप्त विंडोमधून बाहेर पडा",
    title: "गुप्त विंडो",
    body: "खाते आणि सेटिंग्ज नेहमीप्रमाणे आहेत, आधीच्या संभाषणांचा समावेश होणार नाही. हे विंडो बंद केल्यावर या चॅटची नोंद राहणार नाही.",
    dismiss: "गुप्त बॅनर बंद करा",
    errorTitle: "गुप्त विंडो उघडता आली नाही",
    errorBody: "पुन्हा प्रयत्न करा. तरी न झाल्यास Codex बंद करून पुन्हा उघडा.",
    errorRetry: "पुन्हा प्रयत्न करा",
    errorClose: "बंद करा"
  },
  "ms-MY": {
    open: "Buka tetingkap inkognito",
    exit: "Keluar dari tetingkap inkognito",
    title: "Tetingkap inkognito",
    body: "Akaun dan tetapan sama seperti biasa, tanpa perbualan terdahulu. Tutup tetingkap ini dan sembang ini tidak akan meninggalkan rekod.",
    dismiss: "Tutup sepanduk inkognito",
    errorTitle: "Tidak dapat membuka tetingkap inkognito",
    errorBody: "Cuba lagi. Jika masih gagal, tutup Codex lalu buka semula.",
    errorRetry: "Cuba lagi",
    errorClose: "Tutup"
  },
  "my-MM": {
    open: "လျှို့ဝှက်ဝင်းဒိုးဖွင့်ရန်",
    exit: "လျှို့ဝှက်ဝင်းဒိုးမှ ထွက်ရန်",
    title: "လျှို့ဝှက်ဝင်းဒိုး",
    body: "အကောင့်နှင့် ဆက်တင်များသည် ပုံမှန်အတိုင်းဖြစ်ပြီး ယခင်စကားဝိုင်းများ မပါဝင်ပါ။ ဤဝင်းဒိုးကို ပိတ်လိုက်လျှင် ဤချတ်မှတ်တမ်း ကျန်မည်မဟုတ်ပါ။",
    dismiss: "လျှို့ဝှက်နဖူးစည်းကို ပိတ်ရန်",
    errorTitle: "လျှို့ဝှက်ဝင်းဒိုး ဖွင့်မရပါ",
    errorBody: "ထပ်ကြိုးစားပါ။ မရသေးရင် Codex ကို ပိတ်ပြီး ပြန်ဖွင့်ပါ။",
    errorRetry: "ထပ်ကြိုးစားရန်",
    errorClose: "ပိတ်ရန်"
  },
  "nb-NO": {
    open: "Åpne inkognitovindu",
    exit: "Avslutt inkognitovindu",
    title: "Inkognitovindu",
    body: "Konto og innstillinger er som vanlig, uten tidligere samtaler. Lukker du dette vinduet, etterlater ikke denne chatten noen oppføring.",
    dismiss: "Lukk inkognitobanneret",
    errorTitle: "Kunne ikke åpne inkognitovinduet",
    errorBody: "Prøv igjen. Hvis det fortsatt feiler, avslutt Codex og åpne det på nytt.",
    errorRetry: "Prøv igjen",
    errorClose: "Lukk"
  },
  "nl-NL": {
    open: "Incognitovenster openen",
    exit: "Incognitovenster sluiten",
    title: "Incognitovenster",
    body: "Account en instellingen zijn hetzelfde als anders, zonder eerdere gesprekken. Sluit dit venster en deze chat laat geen record achter.",
    dismiss: "Incognitobanner sluiten",
    errorTitle: "Incognitovenster kon niet worden geopend",
    errorBody: "Probeer het opnieuw. Lukt het nog niet, sluit Codex en open het opnieuw.",
    errorRetry: "Opnieuw proberen",
    errorClose: "Sluiten"
  },
  pa: {
    open: "ਇਨਕੌਗਨੀਟੋ ਵਿੰਡੋ ਖੋਲ੍ਹੋ",
    exit: "ਇਨਕੌਗਨੀਟੋ ਵਿੰਡੋ ਤੋਂ ਬਾਹਰ ਜਾਓ",
    title: "ਇਨਕੌਗਨੀਟੋ ਵਿੰਡੋ",
    body: "ਖਾਤਾ ਅਤੇ ਸੈਟਿੰਗਾਂ ਆਮ ਵਾਂਗ ਹਨ, ਪਿਛਲੀਆਂ ਗੱਲਾਂ ਨਹੀਂ ਆਉਣਗੀਆਂ। ਇਹ ਵਿੰਡੋ ਬੰਦ ਕਰਨ ਤੇ ਇਸ ਚੈਟ ਦਾ ਰਿਕਾਰਡ ਨਹੀਂ ਰਹੇਗਾ।",
    dismiss: "ਇਨਕੌਗਨੀਟੋ ਬੈਨਰ ਬੰਦ ਕਰੋ",
    errorTitle: "ਇਨਕੌਗਨੀਟੋ ਵਿੰਡੋ ਨਹੀਂ ਖੁੱਲ੍ਹੀ",
    errorBody: "ਫਿਰ ਕੋਸ਼ਿਸ਼ ਕਰੋ। ਫਿਰ ਵੀ ਨਾ ਖੁੱਲ੍ਹੇ ਤਾਂ Codex ਬੰਦ ਕਰਕੇ ਮੁੜ ਖੋਲ੍ਹੋ।",
    errorRetry: "ਫਿਰ ਕੋਸ਼ਿਸ਼ ਕਰੋ",
    errorClose: "ਬੰਦ ਕਰੋ"
  },
  "pl-PL": {
    open: "Otwórz okno incognito",
    exit: "Zamknij okno incognito",
    title: "Okno incognito",
    body: "Konto i ustawienia są jak zwykle, bez wcześniejszych rozmów. Zamknij to okno, a ta rozmowa nie pozostawi zapisu.",
    dismiss: "Zamknij baner incognito",
    errorTitle: "Nie udało się otworzyć okna incognito",
    errorBody: "Spróbuj ponownie. Jeśli nadal nie działa, zamknij Codex i otwórz go znowu.",
    errorRetry: "Spróbuj ponownie",
    errorClose: "Zamknij"
  },
  "pt-BR": {
    open: "Abrir janela anônima",
    exit: "Sair da janela anônima",
    title: "Janela anônima",
    body: "A conta e as configurações são as de sempre, sem conversas anteriores. Feche esta janela e este chat não deixará registro.",
    dismiss: "Fechar o banner da janela anônima",
    errorTitle: "Não foi possível abrir a janela anônima",
    errorBody: "Tente de novo. Se ainda falhar, saia do Codex e abra novamente.",
    errorRetry: "Tentar de novo",
    errorClose: "Fechar"
  },
  "pt-PT": {
    open: "Abrir janela de navegação privada",
    exit: "Sair da janela de navegação privada",
    title: "Janela de navegação privada",
    body: "A conta e as definições são as do costume, sem conversas anteriores. Feche esta janela e esta conversa não deixará registo.",
    dismiss: "Fechar o banner de navegação privada",
    errorTitle: "Não foi possível abrir a janela de navegação privada",
    errorBody: "Tente novamente. Se continuar a falhar, saia do Codex e volte a abri-lo.",
    errorRetry: "Tentar novamente",
    errorClose: "Fechar"
  },
  "ro-RO": {
    open: "Deschide fereastra incognito",
    exit: "Ieși din fereastra incognito",
    title: "Fereastră incognito",
    body: "Contul și setările sunt ca de obicei, fără conversațiile anterioare. Dacă închizi această fereastră, acest chat nu va lăsa nicio înregistrare.",
    dismiss: "Închide bannerul incognito",
    errorTitle: "Nu s-a putut deschide fereastra incognito",
    errorBody: "Încearcă din nou. Dacă tot nu merge, închide Codex și deschide-l din nou.",
    errorRetry: "Încearcă din nou",
    errorClose: "Închide"
  },
  "ru-RU": {
    open: "Открыть окно инкогнито",
    exit: "Выйти из окна инкогнито",
    title: "Окно инкогнито",
    body: "Аккаунт и настройки как обычно, без прошлых диалогов. Закройте это окно — и этот чат не оставит записи.",
    dismiss: "Закрыть баннер окна инкогнито",
    errorTitle: "Не удалось открыть окно инкогнито",
    errorBody: "Попробуйте ещё раз. Если снова не получится, закройте Codex и откройте его заново.",
    errorRetry: "Повторить",
    errorClose: "Закрыть"
  },
  "sk-SK": {
    open: "Otvoriť anonymné okno",
    exit: "Ukončiť anonymné okno",
    title: "Anonymné okno",
    body: "Účet a nastavenia sú ako zvyčajne, bez predchádzajúcich konverzácií. Ak toto okno zatvoríte, tento chat nezanechá záznam.",
    dismiss: "Zavrieť banner anonymného okna",
    errorTitle: "Anonymné okno sa nepodarilo otvoriť",
    errorBody: "Skúste znova. Ak to stále nejde, ukončite Codex a otvorte ho znova.",
    errorRetry: "Skúsiť znova",
    errorClose: "Zavrieť"
  },
  "sl-SI": {
    open: "Odpri okno brez beleženja",
    exit: "Zapusti okno brez beleženja",
    title: "Okno brez beleženja",
    body: "Račun in nastavitve so kot običajno, brez prejšnjih pogovorov. Če zaprete to okno, ta klepet ne pusti zapisa.",
    dismiss: "Zapri pasico okna brez beleženja",
    errorTitle: "Okna brez beleženja ni bilo mogoče odpreti",
    errorBody: "Poskusite znova. Če še vedno ne deluje, zaprite Codex in ga znova odprite.",
    errorRetry: "Poskusi znova",
    errorClose: "Zapri"
  },
  "so-SO": {
    open: "Fur daaqadda qarsoodiga",
    exit: "Ka bax daaqadda qarsoodiga",
    title: "Daaqadda qarsoodiga",
    body: "Akoonka iyo dejinta waa sidii caadiga ahayd, wadahadalladii hore ma imanayaan. Daaqaddan xir, sheekadani ma reebi doonto diiwaan.",
    dismiss: "Xir calanka daaqadda qarsoodiga",
    errorTitle: "Daaqadda qarsoodiga lama furi karin",
    errorBody: "Isku day mar kale. Haddii weli ay fashilanto, xir Codex ka dibna fur.",
    errorRetry: "Isku day mar kale",
    errorClose: "Xir"
  },
  "sq-AL": {
    open: "Hap dritaren inkognito",
    exit: "Dil nga dritarja inkognito",
    title: "Dritare inkognito",
    body: "Llogaria dhe cilësimet janë si zakonisht, pa bisedat e mëparshme. Mbylleni këtë dritare dhe ky bisedim nuk do të lërë regjistër.",
    dismiss: "Mbyll banderolën inkognito",
    errorTitle: "Nuk u hap dritarja inkognito",
    errorBody: "Provo sërish. Nëse prapë dështon, mbyll Codex dhe hapë përsëri.",
    errorRetry: "Provo sërish",
    errorClose: "Mbyll"
  },
  "sr-RS": {
    open: "Отвори прозор инкогнито",
    exit: "Изађи из прозора инкогнито",
    title: "Прозор инкогнито",
    body: "Налог и подешавања су као обично, без претходних разговора. Ако затворите овај прозор, овај ћаскање неће оставити запис.",
    dismiss: "Затвори банер инкогнито",
    errorTitle: "Није могуће отворити прозор инкогнито",
    errorBody: "Покушајте поново. Ако и даље не ради, затворите Codex и отворите га поново.",
    errorRetry: "Покушај поново",
    errorClose: "Затвори"
  },
  "sv-SE": {
    open: "Öppna inkognitofönster",
    exit: "Avsluta inkognitofönster",
    title: "Inkognitofönster",
    body: "Konto och inställningar är som vanligt, utan tidigare samtal. Stänger du det här fönstret lämnar den här chatten ingen post.",
    dismiss: "Stäng inkognitobannern",
    errorTitle: "Kunde inte öppna inkognitofönstret",
    errorBody: "Försök igen. Om det fortfarande inte går, avsluta Codex och öppna det igen.",
    errorRetry: "Försök igen",
    errorClose: "Stäng"
  },
  "sw-TZ": {
    open: "Fungua dirisha la faragha",
    exit: "Toka kwenye dirisha la faragha",
    title: "Dirisha la faragha",
    body: "Akaunti na mipangilio ni kama kawaida, bila mazungumzo ya awali. Funga dirisha hili na gumzo hili halitaacha rekodi.",
    dismiss: "Funga bango la dirisha la faragha",
    errorTitle: "Imeshindwa kufungua dirisha la faragha",
    errorBody: "Jaribu tena. Ikiwa bado itashindwa, funga Codex kisha uifungue tena.",
    errorRetry: "Jaribu tena",
    errorClose: "Funga"
  },
  "ta-IN": {
    open: "மறைநிலை சாளரத்தைத் திற",
    exit: "மறைநிலை சாளரத்திலிருந்து வெளியேறு",
    title: "மறைநிலை சாளரம்",
    body: "கணக்கும் அமைப்புகளும் வழக்கம் போலவே; முந்தைய உரையாடல்கள் வராது. இந்தச் சாளரத்தை மூடினால் இந்த அரட்டையின் பதிவு இருக்காது.",
    dismiss: "மறைநிலை பதாகையை மூடு",
    errorTitle: "மறைநிலை சாளரத்தைத் திறக்க முடியவில்லை",
    errorBody: "மீண்டும் முயலவும். இன்னும் திறக்கவில்லை என்றால் Codex-ஐ மூடி மீண்டும் திறக்கவும்.",
    errorRetry: "மீண்டும் முயலவும்",
    errorClose: "மூடு"
  },
  "te-IN": {
    open: "అజ్ఞాత విండోను తెరవండి",
    exit: "అజ్ఞాత విండో నుంచి నిష్క్రమించండి",
    title: "అజ్ఞాత విండో",
    body: "ఖాతా, సెట్టింగ్‌లు ఎప్పటిలాగే ఉంటాయి; మునుపటి సంభాషణలు రావు. ఈ విండో మూస్తే ఈ చాట్ రికార్డు ఉండదు.",
    dismiss: "అజ్ఞాత బ్యానర్‌ను మూసివేయండి",
    errorTitle: "అజ్ఞాత విండో తెరవలేకపోయాం",
    errorBody: "మళ్లీ ప్రయత్నించండి. ఇంకా కాకపోతే Codex ను మూసి మళ్లీ తెరవండి.",
    errorRetry: "మళ్లీ ప్రయత్నించండి",
    errorClose: "మూసివేయి"
  },
  "th-TH": {
    open: "เปิดหน้าต่างไม่ระบุตัวตน",
    exit: "ออกจากหน้าต่างไม่ระบุตัวตน",
    title: "หน้าต่างไม่ระบุตัวตน",
    body: "บัญชีและการตั้งค่าเหมือนเดิม จะไม่ดึงบทสนทนาก่อนหน้า ปิดหน้าต่างนี้แล้วแชทนี้จะไม่เหลือบันทึก",
    dismiss: "ปิดแบนเนอร์หน้าต่างไม่ระบุตัวตน",
    errorTitle: "เปิดหน้าต่างไม่ระบุตัวตนไม่ได้",
    errorBody: "ลองอีกครั้ง หากยังไม่ได้ ให้ปิด Codex แล้วเปิดใหม่",
    errorRetry: "ลองอีกครั้ง",
    errorClose: "ปิด"
  },
  tl: {
    open: "Buksan ang incognito window",
    exit: "Lumabas sa incognito window",
    title: "Incognito window",
    body: "Pareho pa rin ang account at settings, walang dating usapan. Isara ang window na ito at hindi mag-iiwan ng record ang chat na ito.",
    dismiss: "Isara ang incognito banner",
    errorTitle: "Hindi mabuksan ang incognito window",
    errorBody: "Subukan ulit. Kung hindi pa rin, isara ang Codex at buksan ulit.",
    errorRetry: "Subukan ulit",
    errorClose: "Isara"
  },
  "tr-TR": {
    open: "Gizli pencere aç",
    exit: "Gizli pencereden çık",
    title: "Gizli pencere",
    body: "Hesap ve ayarlar her zamanki gibi; önceki sohbetler gelmez. Bu pencereyi kapatırsanız bu sohbet kayıt bırakmaz.",
    dismiss: "Gizli pencere banner’ını kapat",
    errorTitle: "Gizli pencere açılamadı",
    errorBody: "Yeniden deneyin. Hâlâ açılmazsa Codex’i kapatıp tekrar açın.",
    errorRetry: "Yeniden dene",
    errorClose: "Kapat"
  },
  "uk-UA": {
    open: "Відкрити вікно інкогніто",
    exit: "Вийти з вікна інкогніто",
    title: "Вікно інкогніто",
    body: "Обліковий запис і налаштування як завжди, без попередніх розмов. Закрийте це вікно — і цей чат не залишить запису.",
    dismiss: "Закрити банер вікна інкогніто",
    errorTitle: "Не вдалося відкрити вікно інкогніто",
    errorBody: "Спробуйте ще раз. Якщо знову не вийде, закрийте Codex і відкрийте його знову.",
    errorRetry: "Спробувати ще раз",
    errorClose: "Закрити"
  },
  ur: {
    open: "خفیہ ونڈو کھولیں",
    exit: "خفیہ ونڈو سے باہر نکلیں",
    title: "خفیہ ونڈو",
    body: "اکاؤنٹ اور سیٹنگز ویسی ہی ہیں، پچھلی گفتگو نہیں آئے گی۔ یہ ونڈو بند کرنے پر اس چیٹ کا ریکارڈ نہیں رہے گا۔",
    dismiss: "خفیہ بینر بند کریں",
    errorTitle: "خفیہ ونڈو نہیں کھل سکی",
    errorBody: "دوبارہ کوشش کریں۔ پھر بھی نہ کھلے تو Codex بند کر کے دوبارہ کھولیں۔",
    errorRetry: "دوبارہ کوشش کریں",
    errorClose: "بند کریں"
  },
  "vi-VN": {
    open: "Mở cửa sổ ẩn danh",
    exit: "Thoát cửa sổ ẩn danh",
    title: "Cửa sổ ẩn danh",
    body: "Tài khoản và cài đặt như bình thường, không mang theo hội thoại cũ. Đóng cửa sổ này thì cuộc trò chuyện này sẽ không để lại bản ghi.",
    dismiss: "Đóng banner cửa sổ ẩn danh",
    errorTitle: "Không mở được cửa sổ ẩn danh",
    errorBody: "Thử lại. Nếu vẫn không được, hãy thoát Codex rồi mở lại.",
    errorRetry: "Thử lại",
    errorClose: "Đóng"
  }
};

// src/runtime/incognito-copy.ts
var CORE_COPY = {
  en: {
    open: "Open incognito window",
    exit: "Exit incognito window",
    title: "Incognito window",
    body: "Same account and settings as usual, without earlier chats. This conversation will not show up in your everyday chat list. Temporary data is removed after a normal exit.",
    dismiss: "Dismiss incognito banner",
    errorTitle: "Couldn’t open the incognito window",
    errorBody: "Try again. If it still fails, quit Codex and open it again.",
    errorRetry: "Try again",
    errorClose: "Close"
  },
  "zh-CN": {
    open: "打开无痕窗口",
    exit: "退出无痕窗口",
    title: "无痕窗口",
    body: "账号和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。",
    dismiss: "关闭无痕窗口横幅",
    errorTitle: "无法打开无痕窗口",
    errorBody: "再试一次。如果还是不行，先退出 Codex 再打开。",
    errorRetry: "再试一次",
    errorClose: "关闭"
  },
  "zh-HK": {
    open: "開啟無痕視窗",
    exit: "離開無痕視窗",
    title: "無痕視窗",
    body: "帳戶和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉無痕視窗橫額",
    errorTitle: "無法開啟無痕視窗",
    errorBody: "再試一次。如果仍然不行，先退出 Codex 再開。",
    errorRetry: "再試一次",
    errorClose: "關閉"
  },
  "zh-TW": {
    open: "開啟無痕視窗",
    exit: "離開無痕視窗",
    title: "無痕視窗",
    body: "帳號和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉無痕視窗橫幅",
    errorTitle: "無法開啟無痕視窗",
    errorBody: "再試一次。如果還是不行，先退出 Codex 再開啟。",
    errorRetry: "再試一次",
    errorClose: "關閉"
  }
};
var COPY2 = {
  ...COPY,
  ...CORE_COPY
};
var LANGUAGE_DEFAULT_OVERRIDES = {
  es: "es-419",
  fr: "fr-FR",
  no: "nb-NO",
  pt: "pt-BR"
};
function resolveLocale(raw) {
  const normalized = raw.trim().replaceAll("_", "-");
  if (!normalized)
    return "en";
  if (COPY2[normalized])
    return normalized;
  const lower = normalized.toLowerCase();
  const exact = Object.keys(COPY2).find((key) => key.toLowerCase() === lower);
  if (exact)
    return exact;
  if (lower.startsWith("zh-hant-hk") || lower.startsWith("zh-hk"))
    return "zh-HK";
  if (lower.startsWith("zh-hant") || lower.startsWith("zh-tw"))
    return "zh-TW";
  if (lower.startsWith("zh"))
    return "zh-CN";
  if (lower === "en" || lower.startsWith("en-"))
    return "en";
  const language = lower.split("-")[0] ?? "en";
  if (COPY2[language])
    return language;
  const defaultOverride = LANGUAGE_DEFAULT_OVERRIDES[language];
  if (defaultOverride) {
    return defaultOverride;
  }
  const regional = Object.keys(COPY2).find((key) => key.toLowerCase().startsWith(`${language}-`));
  return regional ?? "en";
}
function translate(locale, key) {
  const resolved = resolveLocale(locale);
  if (key === "body")
    return CORE_COPY[resolved]?.body ?? CORE_COPY.en.body;
  return COPY2[resolved]?.[key] ?? COPY2.en[key];
}

// node_modules/blobatar/dist/uri.js
function L({ l: t, c: n, h: r }) {
  let o = r * Math.PI / 180, a = n * Math.cos(o), e = n * Math.sin(o), s = t + 0.3963377774 * a + 0.2158037573 * e, c = t - 0.1055613458 * a - 0.0638541728 * e, i = t - 0.0894841775 * a - 1.291485548 * e, m = s * s * s, l = c * c * c, u = i * i * i;
  return [4.0767416621 * m - 3.3077115913 * l + 0.2309699292 * u, -1.2684380046 * m + 2.6097574011 * l - 0.3413193965 * u, -0.0041960863 * m - 0.7034186147 * l + 1.707614701 * u];
}
var H = (t) => t.every((n) => n >= -0.0001 && n <= 1.0001);
function Z(t) {
  let n = L(t);
  if (!H(n)) {
    let r = 0, o = t.c;
    for (let a = 0;a < 12; a++) {
      let e = (r + o) / 2;
      if (H(L({ ...t, c: e })))
        r = e;
      else
        o = e;
    }
    n = L({ ...t, c: r });
  }
  return n.map((r) => Math.min(1, Math.max(0, r)));
}
function _(t) {
  let [n, r, o] = Z(t);
  return 0.2126 * n + 0.7152 * r + 0.0722 * o;
}
function O(t, n) {
  let r = _(t), o = _(n);
  return (Math.max(r, o) + 0.05) / (Math.min(r, o) + 0.05);
}
function T(t, n, r) {
  if (O(t, n) >= r)
    return t;
  let o = t.l >= n.l ? 1 : -1;
  for (let s of [o, -o]) {
    let c = { ...t };
    for (let i = 0;i < 60; i++) {
      if (c.l = Math.min(1, Math.max(0, c.l + s * 0.02)), O(c, n) >= r)
        return c;
      if (c.l === 0 || c.l === 1)
        break;
    }
  }
  let a = { ...t, l: 0, c: 0 }, e = { ...t, l: 1, c: 0 };
  return O(a, n) >= O(e, n) ? a : e;
}
function P(t) {
  return "#" + Z(t).map((n) => {
    let r = n <= 0.0031308 ? 12.92 * n : 1.055 * Math.pow(n, 0.4166666666666667) - 0.055;
    return Math.round(r * 255).toString(16).padStart(2, "0");
  }).join("");
}
var D = [[0.2, { l: 0.86, c: 0.085 }], [0.36, { l: 0.9, c: 0.028 }], [0.62, { l: 0.73, c: 0.135 }], [0.8, { l: 0.62, c: 0.165 }], [0.93, { l: 0.87, c: 0.16 }], [1, { l: 0.34, c: 0.035 }]];
var Tt = (t) => D.find(([n]) => t < n)?.[1] ?? D[0][1];
var N = { l: 0.145, c: 0, h: 0 };
var V = 1.5;
var St = (t, n) => {
  let r = Tt(n), o = T({ l: r.l, c: r.c, h: t }, N, V);
  return { bg: { l: 0.965, c: 0.01, h: t }, head: o, eye: o.l >= 0.5 ? { l: 0.17, c: 0.02, h: t } : { l: 0.97, c: 0.012, h: t } };
};
var Et = [["head", "bg", 1.25], ["eye", "head", 4.5]];
function Lt(t, n = true, r = 0) {
  let o = St(t, r);
  if (n)
    for (let [a, e, s] of Et)
      o[a] = T(o[a], o[e], s);
  return o;
}
function K(t, n = true, r = 0) {
  let o = Lt(t, n, r), a = {};
  for (let e in o)
    a[e] = P(o[e]);
  return a;
}
var y = (t) => {
  let n = Math.round(t * 100) / 100;
  return Object.is(n, -0) ? "0" : String(n);
};
function B({ cx: t, cy: n, rx: r, ry: o, n: a = 4, rot: e = 0 }) {
  let s = Math.min(1, (8 * Math.pow(2, -1 / a) - 4) / 3), c = r, i = o, m = c * s, l = i * s, u = [[c, 0], [c, l], [m, i], [0, i], [-m, i], [-c, l], [-c, 0], [-c, -l], [-m, -i], [0, -i], [m, -i], [c, -l], [c, 0]], b = e * Math.PI / 180, p = Math.cos(b), d = Math.sin(b), h = (x) => {
    let [g, M] = u[x];
    return `${y(t + g * p - M * d)} ${y(n + g * d + M * p)}`;
  }, f = `M${h(0)}`;
  for (let x = 1;x < 13; x += 3)
    f += `C${h(x)} ${h(x + 1)} ${h(x + 2)}`;
  return f + "Z";
}
function Q(t, n, r, o, a, e = 0) {
  let s = a.length, c = e * Math.PI / 180, i = a.map((u, b) => {
    let p = c + 2 * Math.PI * b / s;
    return [t + r * u * Math.cos(p), n + o * u * Math.sin(p)];
  }), m = (u) => i[(u % s + s) % s], l = `M${y(m(0)[0])} ${y(m(0)[1])}`;
  for (let u = 0;u < s; u++) {
    let [b, p] = m(u - 1), [d, h] = m(u), [f, x] = m(u + 1), [g, M] = m(u + 2);
    l += `C${y(d + (f - b) / 6)} ${y(h + (x - p) / 6)} ${y(f - (g - d) / 6)} ${y(x - (M - h) / 6)} ${y(f)} ${y(x)}`;
  }
  return l + "Z";
}
function X({ cx: t, cy: n, rx: r, ry: o, sides: a, round: e = 0.3, rot: s = 0 }) {
  let c = e > 0 ? e < 1 ? e / 2 : 0.5 : 0, i = s * Math.PI / 180 - Math.PI / 2, m = Array.from({ length: a }, (p, d) => {
    let h = i + 2 * Math.PI * d / a;
    return [t + r * Math.cos(h), n + o * Math.sin(h)];
  }), l = (p) => m[(p % a + a) % a], u = (p, d) => {
    let [h, f] = l(p), [x, g] = l(d);
    return `${y(h + (x - h) * c)} ${y(f + (g - f) * c)}`;
  }, b = `M${u(0, -1)}`;
  for (let p = 0;p < a; p++) {
    let [d, h] = l(p);
    if (b += `Q${y(d)} ${y(h)} ${u(p, p + 1)}`, c < 0.5)
      b += `L${u(p + 1, p)}`;
  }
  return b + "Z";
}
function Y(t, n, r, o) {
  let a = y(t - r), e = y(t + r);
  return `M${a} ${y(n - o)}H${e}V${y(n + o)}H${a}Z`;
}
function G(t, n, r, o, a) {
  let e = Math.max(1.05, a), s = r * Math.sqrt(1 - 1 / (e * e)), c = n - o / e, i = n - e * o, m = s * 0.14, l = c + 0.86 * (i - c);
  return `M${y(t - s)} ${y(c)}L${y(t - m)} ${y(l)}Q${y(t)} ${y(i)} ${y(t + m)} ${y(l)}L${y(t + s)} ${y(c)}Z`;
}
function w(t, n) {
  for (let r = 0;r < n.length; r++)
    t = Math.imul(t ^ n[r], 3432918353), t = t << 13 | t >>> 19;
  return t;
}
function wt(t) {
  return t = Math.imul(t ^ t >>> 16, 2246822507), t = Math.imul(t ^ t >>> 13, 3266489909), (t ^ t >>> 16) >>> 0;
}
var J = new TextEncoder;
function vt(t) {
  return t.normalize("NFC").trim().toLowerCase();
}
function W(t, n = true) {
  let r = n ? vt(t) : t;
  return w(1779033703 ^ r.length, J.encode(r));
}
function v(t, n) {
  return wt(w(w(t, Uint8Array.of(255)), J.encode(n))) / 4294967296;
}
function tt(t, n = true, r) {
  let o = W(t, n), a = (e) => {
    let s = r?.[e], c = Array.isArray(s) ? s[Math.floor(v(o, e) * s.length)] : s;
    return c === undefined ? v(o, e) : c > 0 ? c < 1 ? c : 0.999999 : 0;
  };
  return a.num = (e, s, c) => s + a(e) * (c - s), a.int = (e, s, c) => s + Math.floor(a(e) * (c - s + 1)), a.pick = (e, s) => s[Math.floor(a(e) * s.length)], a.bool = (e, s = 0.5) => a(e) < s, a.jitter = (e, s) => (a(e) * 2 - 1) * s, a;
}
function nt(t, n, r) {
  let o = n.expression;
  if (r || !o)
    return { l: t, wrap: "" };
  return o.bake(t, o.p);
}
var At = (t, n) => n?.tint ? n.tint(t, n.p) : t;
var rt = (t, n) => n ? `<g transform="${n}">${t}</g>` : t;
var It = (t) => t.replace(/[&<>]/g, (n) => n === "&" ? "&amp;" : n === "<" ? "&lt;" : "&gt;");
function S(t, n) {
  let r = tt(t, n.normalize ?? true, n.traits);
  return { t: r, palette: { ...K(n.hue ?? r.num("hue", 0, 360), n.contrast ?? true, n.tone ?? r("tone")), ...n.palette } };
}
var Rt = (t) => t.title ? `<title>${It(t.title)}</title>` : "";
function et(t, n, r) {
  let o = n.background ?? t.background;
  if (o === false)
    return;
  return { d: o === "square" ? "M0 0H100V100H0Z" : B({ cx: 50, cy: 50, rx: 50, ry: 50, n: o === "circle" ? 2 : 6 }), fill: r.bg };
}
var Ft = (t) => t ? `<path d="${t.d}" fill="${t.fill}"/>` : "";
function ot(t) {
  return (n, r = {}) => {
    let { t: o, palette: a } = S(n, r), e = At(a, r.expression), s = r.size ? ` width="${r.size}" height="${r.size}"` : "", c = nt(t.layout(o), r), i = Rt(r) + Ft(et(t, r, e)) + rt(t.render(c.l, e), c.wrap);
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"${s}>${i}</svg>`;
  };
}
var st = (t, n, r) => {
  let o = n.rx, a = t.num("eye.rx", 0.075, 0.105) * o, e = t.num("eye.ratio", 1.9, 3.2), s = t.num("eye.scale", 0.78, 1.24), c = t.num("eye.stretch", 0.85, 1.18), i = t.num("eye.gap", 0.1, 0.24) * o, m = a * Math.max(1, s), l = a * e * Math.max(1, s * c), u = m + o * 0.03 + i, b = t.jitter("gaze.x", 0.09) * r.rx, p = t.num("gaze.y", -0.2, 0.08) * r.ry, d = t.jitter("eye.dy", 0.04) * r.ry, h = Math.hypot(m, l), f = Math.hypot((Math.abs(b) + u + h) / r.rx, (Math.abs(p) + Math.abs(d) + h) / r.ry), x = f > 0.9 ? 0.9 / f : 1, g = a * x, M = g * e, I = u * x, kt = Math.max(0, Math.min(1, i / l)), Ot = Math.min(12, Math.asin(kt) * 180 / Math.PI), R = t.num("eye.lean", -1, 1) * Ot, Pt = Math.max(-12, Math.min(12, R + t.jitter("eye.lean2", 3.5))), F = r.cx + b * x, j = r.cy + p * x;
  return [{ cx: F - I, cy: j, rx: g, ry: M, n: t.num("eye.n", 3.5, 6), rot: R }, { cx: F + I, cy: j + d * x, rx: g * s, ry: M * s * c, n: t.num("eye.n", 3.5, 6), rot: Pt }];
};
function ct(t, n) {
  let r = (e) => (t.find(([, s]) => e < s) ?? t[t.length - 1])[0];
  function o(e) {
    let s = r(e("shape")), c = e.num("body.r", 31, 38) * s.core, i = { cx: 50 + e.jitter("body.x", 1.5), cy: 50 + e.jitter("body.y", 1.5), rx: c, ry: c * e.num("body.ratio", 0.92, 1.08), n: e.num("body.n", 1.9, 2.5), rot: 0, radii: Array.from({ length: e.int("body.pts", 6, 8) }, (u, b) => 1 + e.jitter(`body.r${b}`, 0.16)) };
    s.body?.(e, i);
    let m = s.face?.(i) ?? i, l = { petals: [], extra: [] };
    return s.decorate?.(e, i, l), { shape: s.name, draw: s.path, body: i, face: m, petals: l.petals, extra: l.extra, eyes: n(e, i, m) };
  }
  function a(e, s, c) {
    let i = (u) => Math.round(u * 100) / 100, m = (u, b) => {
      let p = `<path d="${B(u)}"/>`;
      return c ? `<g class="mo-eye" style="--mo-wrap:${b ? 1 : -1};--mo-lean:${i(u.rot)};transform-origin:${i(u.cx)}px ${i(u.cy)}px">${p}</g>` : p;
    }, l = `<g fill="${s.head}">` + e.petals.map((u) => `<circle cx="${i(u.cx)}" cy="${i(u.cy)}" r="${i(u.r)}"/>`).join("") + e.extra.map((u) => `<path d="${u}"/>`).join("") + `<path d="${e.draw ? e.draw(e.body) : B(e.body)}"/></g><g fill="${s.eye}"${c ? ' class="mo-eyes"' : ""}>` + e.eyes.map(m).join("") + "</g>";
    return c ? `<g class="mo-breathe"><g class="mo-bob">${l}</g></g>` : l;
  }
  return { layout: o, render: a, background: false };
}
var it = (t) => X(t);
var ut = (t) => Q(t.cx, t.cy, t.rx, t.ry, t.radii, t.rot);
var A = (t) => (n) => ({ cx: n.cx, cy: n.cy, rx: n.rx * t, ry: n.ry * t });
var mt = (t) => A(Math.min(...t.radii) * 0.95)(t);
var jt = (t) => A(0.84)(t);
var lt = { name: "round", core: 1 };
var pt = { name: "organic", core: 0.98, path: ut, face: mt };
var yt = { name: "boxy", core: 0.86, body: (t, n) => {
  n.n = t.num("body.n", 3.4, 6), n.rot = t.num("body.rot", -20, 20);
} };
var bt = { name: "capsule", core: 1.02, body: (t, n) => {
  n.ry *= t.num("capsule.squat", 0.55, 0.68);
}, face: A(0.94), decorate: (t, n, r) => {
  for (let o of [-1, 1])
    r.petals.push({ cx: n.cx + o * (n.rx - n.ry), cy: n.cy, r: n.ry });
}, path: (t) => Y(t.cx, t.cy, t.rx - t.ry, t.ry) };
var xt = { name: "nub", core: 0.88, decorate: (t, n, r) => {
  let o = t.int("nub.n", 1, 2);
  for (let a = 0;a < o; a++) {
    let e = t.num(`nub.a${a}`, 0, 2 * Math.PI);
    r.petals.push({ cx: n.cx + Math.cos(e) * n.rx * 0.88, cy: n.cy + Math.sin(e) * n.rx * 0.88, r: n.rx * t.num(`nub.r${a}`, 0.24, 0.4) });
  }
} };
var ht = { name: "cloud", core: 0.78, face: mt, path: ut, decorate: (t, n, r) => {
  let o = t.int("cloud.n", 4, 6);
  for (let a = 0;a < o; a++) {
    let e = Math.PI + Math.PI * (a + 0.5) / o;
    r.petals.push({ cx: n.cx + Math.cos(e) * n.rx * 0.8, cy: n.cy + Math.sin(e) * n.rx * 0.5, r: n.rx * t.num(`cloud.r${a}`, 0.44, 0.62) });
  }
} };
var dt = { name: "droplet", core: 0.78, body: (t, n) => {
  n.cy += 0.22 * n.ry, n.n = 2;
}, face: (t) => ({ cx: t.cx, cy: t.cy + t.ry * 0.05, rx: t.rx * 0.88, ry: t.ry * 0.88 }), decorate: (t, n, r) => {
  r.extra.push(G(n.cx, n.cy, n.rx, n.ry, t.num("droplet.tip", 1.4, 1.65)));
} };
var ft = { name: "hexagon", core: 1.05, path: it, face: jt, body: (t, n) => {
  n.sides = 6, n.rot = t.num("body.rot", -12, 12), n.round = t.num("poly.round", 0.24, 0.5);
} };
var gt = { name: "sun", core: 0.7, decorate: (t, n, r) => {
  let o = t.int("sun.n", 6, 9), a = n.rx * t.num("sun.dist", 1, 1.08), e = n.rx * t.num("sun.r", 0.2, 0.26), s = t.num("sun.rot", 0, 2 * Math.PI);
  for (let c = 0;c < o; c++) {
    let i = s + 2 * Math.PI * c / o;
    r.petals.push({ cx: n.cx + Math.cos(i) * a, cy: n.cy + Math.sin(i) * a, r: e });
  }
} };
var Mt = { name: "triangle", core: 1.15, path: it, body: (t, n) => {
  n.sides = 3, n.rot = t.num("body.rot", -5, 5), n.round = t.num("poly.round", 0.24, 0.5);
}, face: (t) => ({ cx: t.cx, cy: t.cy + t.ry * 0.1, rx: t.rx * 0.54, ry: t.ry * 0.36 }) };
var Ct = [[lt, 0.22], [pt, 0.48], [yt, 0.6], [bt, 0.7], [xt, 0.79], [ht, 0.86], [dt, 0.915], [ft, 0.95], [gt, 0.98], [Mt, 1]];
var E = ct(Ct, st);
var $t = ot(E);
function hn(t, n) {
  return "data:image/svg+xml," + $t(t, n).replace(/"/g, "'").replace(/[%#<>{}|\\^[\]`]/g, (o) => "%" + o.charCodeAt(0).toString(16).toUpperCase()).replace(/\s+/g, " ");
}

// src/runtime/incognito-profile-mask.ts
var PROFILE_MASK_ATTR = "data-incodex-profile-mask";
var PROFILE_MASK_NAME_ATTR = "data-incodex-profile-mask-name";
var PROFILE_MASK_AVATAR_ATTR = "data-incodex-profile-mask-avatar";
var PROFILE_FOOTER_SELECTOR = 'button.sidebar-item[type="button"]';
var PROFILE_NAME_SELECTOR = ":scope > span.min-w-0.flex-1.truncate";
var PROFILE_AVATAR_SELECTOR = ":scope > img.rounded-full, :scope > span.rounded-full";
var PROFILE_MENU_SELECTOR = '[role="menu"]';
var PROFILE_MENU_ITEM_SELECTOR = '[role="menuitem"]';
var PROFILE_MENU_NAME_SELECTOR = ":scope > div > span.flex-1.min-w-0.truncate";
var PROFILE_MENU_AVATAR_SELECTOR = ":scope > div > span > img.icon-sm.rounded-full, :scope > div > span > span.rounded-full";
var PROFILE_NAME_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-name]";
var PROFILE_AVATAR_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-avatar]";
var PROFILE_NAME_MAX_CHARS = 64;
var PROFILE_AVATAR_MAX_DATA_URL_CHARS = 8 * 1024 * 1024;
function profileMaskConfigured() {
  return window.__incodexProfileMask !== null && window.__incodexProfileMask !== undefined;
}
function readProfileMask() {
  const value = window.__incodexProfileMask;
  if (!value || typeof value.name !== "string" || !value.avatar)
    return null;
  const name = value.name.trim();
  if (!name || [...name].length > PROFILE_NAME_MAX_CHARS || /\p{Cc}/u.test(name)) {
    return null;
  }
  const avatar = value.avatar;
  if (typeof avatar !== "object" || Array.isArray(avatar))
    return null;
  if (avatar.kind === "generated") {
    if (avatar.dataUrl !== undefined)
      return null;
    return { name, avatarDataUrl: hn(name, { background: "circle" }) };
  }
  if (typeof avatar.dataUrl !== "string" || avatar.kind !== undefined)
    return null;
  if (avatar.dataUrl.length > PROFILE_AVATAR_MAX_DATA_URL_CHARS || !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(avatar.dataUrl)) {
    return null;
  }
  return { name, avatarDataUrl: avatar.dataUrl };
}
function findProfileFooter() {
  const candidates = [...document.querySelectorAll(PROFILE_FOOTER_SELECTOR)].filter((element) => element.querySelector(PROFILE_NAME_SELECTOR) && element.querySelector(PROFILE_AVATAR_SELECTOR));
  return candidates.length === 1 ? candidates[0] : null;
}
function findControlledProfileMenu(profileFooter) {
  const menuId = profileFooter.getAttribute("aria-controls");
  if (!menuId)
    return null;
  const menu = document.getElementById(menuId);
  if (!menu?.matches(PROFILE_MENU_SELECTOR))
    return null;
  return menu;
}
function findProfileMenuIdentity(profileMenu) {
  const candidates = [...profileMenu.querySelectorAll(PROFILE_MENU_ITEM_SELECTOR)].filter((item) => item.querySelector(PROFILE_MENU_NAME_SELECTOR) && item.querySelector(PROFILE_MENU_AVATAR_SELECTOR));
  return candidates.length === 1 ? candidates[0] : null;
}
function writeProfileAvatar(avatar, mask) {
  if (avatar instanceof HTMLImageElement) {
    avatar.src = mask.avatarDataUrl;
    avatar.style.objectFit = "cover";
    avatar.style.objectPosition = "center";
    return true;
  }
  if (avatar.matches("span.rounded-full")) {
    avatar.textContent = "";
    avatar.style.backgroundImage = `url("${mask.avatarDataUrl}")`;
    avatar.style.backgroundSize = "cover";
    avatar.style.backgroundPosition = "center";
    return true;
  }
  return false;
}
function ensureIdentityMask(identity, nameSelector, avatarSelector, mask) {
  const elements = profileIdentityElements(identity, nameSelector, avatarSelector);
  if (!elements || !writeProfileAvatar(elements.avatar, mask))
    return false;
  const { avatar, nameHost } = elements;
  nameHost.setAttribute(PROFILE_MASK_NAME_ATTR, "true");
  nameHost.textContent = mask.name;
  avatar.setAttribute(PROFILE_MASK_AVATAR_ATTR, "true");
  identity.setAttribute(PROFILE_MASK_ATTR, "true");
  return true;
}
function profileIdentityElements(identity, nameSelector, avatarSelector) {
  const nameHost = identity.querySelector(PROFILE_NAME_MARKER_SELECTOR) ?? identity.querySelector(nameSelector);
  const avatar = identity.querySelector(PROFILE_AVATAR_MARKER_SELECTOR) ?? identity.querySelector(avatarSelector);
  if (!nameHost || !avatar)
    return null;
  return { avatar, nameHost };
}
function ensureProfileMenuMask(profileFooter, mask) {
  const profileMenu = findControlledProfileMenu(profileFooter);
  const identity = profileMenu ? findProfileMenuIdentity(profileMenu) : null;
  if (!identity)
    return;
  ensureIdentityMask(identity, PROFILE_MENU_NAME_SELECTOR, PROFILE_MENU_AVATAR_SELECTOR, mask);
}
function ensureProfileMask() {
  if (!window.__incodexIncognito || !profileMaskConfigured())
    return;
  const mask = readProfileMask();
  const profileFooter = mask ? findProfileFooter() : null;
  if (!mask || !profileFooter)
    return;
  if (!ensureIdentityMask(profileFooter, PROFILE_NAME_SELECTOR, PROFILE_AVATAR_SELECTOR, mask)) {
    return;
  }
  ensureProfileMenuMask(profileFooter, mask);
}
function profileAvatarHealth(avatar, mask) {
  if (avatar instanceof HTMLImageElement) {
    return avatar.getAttribute("src") === mask.avatarDataUrl && avatar.style.objectFit === "cover" && avatar.style.objectPosition === "center center";
  }
  return avatar.style.backgroundImage === `url("${mask.avatarDataUrl}")` && avatar.style.backgroundSize === "cover" && avatar.style.backgroundPosition === "center center";
}
function profileAvatarDecoded(dataUrl) {
  const current = window.__incodexProfileAvatarDecodeState;
  if (current?.dataUrl === dataUrl)
    return current.status === "ready";
  const state = { dataUrl, probe: null, status: "loading" };
  window.__incodexProfileAvatarDecodeState = state;
  const probe = new Image;
  state.probe = probe;
  function finish(status) {
    if (window.__incodexProfileAvatarDecodeState !== state)
      return;
    state.status = status;
    state.probe = null;
    window.__incodexProfileMaskHealth = profileMaskHealth();
  }
  probe.addEventListener("load", function handleAvatarLoad() {
    finish(probe.naturalWidth > 0 && probe.naturalHeight > 0 ? "ready" : "failed");
  }, { once: true });
  probe.addEventListener("error", function handleAvatarError() {
    finish("failed");
  }, { once: true });
  probe.src = dataUrl;
  return false;
}
function identityMaskHealth(identity, nameSelector, avatarSelector, mask) {
  const elements = profileIdentityElements(identity, nameSelector, avatarSelector);
  if (!elements)
    return false;
  const { avatar, nameHost } = elements;
  return Boolean(identity.getAttribute(PROFILE_MASK_ATTR) === "true" && nameHost.getAttribute(PROFILE_MASK_NAME_ATTR) === "true" && avatar.getAttribute(PROFILE_MASK_AVATAR_ATTR) === "true" && nameHost.textContent === mask.name && profileAvatarHealth(avatar, mask));
}
function profileMaskHealth() {
  if (!profileMaskConfigured())
    return true;
  const mask = readProfileMask();
  const profileFooter = mask ? findProfileFooter() : null;
  if (!mask || !profileAvatarDecoded(mask.avatarDataUrl) || !profileFooter)
    return false;
  if (!identityMaskHealth(profileFooter, PROFILE_NAME_SELECTOR, PROFILE_AVATAR_SELECTOR, mask)) {
    return false;
  }
  const profileMenu = findControlledProfileMenu(profileFooter);
  if (!profileMenu)
    return true;
  const menuIdentity = findProfileMenuIdentity(profileMenu);
  if (!menuIdentity)
    return false;
  return identityMaskHealth(menuIdentity, PROFILE_MENU_NAME_SELECTOR, PROFILE_MENU_AVATAR_SELECTOR, mask);
}
function profileMaskNeedsInject() {
  if (!profileMaskConfigured())
    return false;
  return !profileMaskHealth();
}

// src/runtime/official-tooltip-provider.ts
function createOfficialTooltipTimingBridge(currentTrigger) {
  let activeProvider = null;
  function currentProvider() {
    const trigger = currentTrigger();
    if (!trigger)
      return null;
    return findOfficialTooltipProvider(trigger);
  }
  function deactivate() {
    const provider = activeProvider;
    activeProvider = null;
    if (!provider)
      return;
    try {
      provider.deactivateTooltip(PROVIDER_ID);
    } catch {}
  }
  return {
    resolveDelay(fallbackMs) {
      try {
        const delayMs = currentProvider()?.getOpenDelay(PROVIDER_KEY, fallbackMs) ?? fallbackMs;
        if (!Number.isFinite(delayMs) || delayMs < 0)
          return fallbackMs;
        return delayMs;
      } catch {
        return fallbackMs;
      }
    },
    activate(close) {
      deactivate();
      const provider = currentProvider();
      if (!provider)
        return;
      try {
        provider.activateTooltip(PROVIDER_ID, PROVIDER_KEY, PROVIDER_VARIANT, close);
        activeProvider = provider;
      } catch {
        try {
          provider.deactivateTooltip(PROVIDER_ID);
        } catch {}
      }
    },
    deactivate
  };
}
var REACT_FIBER_PREFIX = "__reactFiber$";
var MAX_FIBER_DEPTH = 64;
var MAX_CONTEXTS_PER_FIBER = 64;
var PROVIDER_ID = "incodex-privacy-toggle";
var PROVIDER_KEY = "default";
var PROVIDER_VARIANT = "tooltip";
function isOfficialTooltipProvider(value) {
  if (typeof value !== "object" || value === null)
    return false;
  const candidate = value;
  return typeof candidate.getOpenDelay === "function" && typeof candidate.activateTooltip === "function" && typeof candidate.clearHoverHandoffLock === "function" && typeof candidate.deactivateTooltip === "function" && typeof candidate.isHoverOpenBlocked === "function" && typeof candidate.registerOpenTooltip === "function" && typeof candidate.registerTooltipDismissHandler === "function" && typeof candidate.setHoverHandoffLockTooltipId === "function";
}
function reactFiber(trigger) {
  const key = Object.keys(trigger).find((name) => name.startsWith(REACT_FIBER_PREFIX));
  if (!key)
    return null;
  return trigger[key] ?? null;
}
function findOfficialTooltipProvider(trigger) {
  let fiber = reactFiber(trigger);
  for (let fiberDepth = 0;fiber && fiberDepth < MAX_FIBER_DEPTH; fiberDepth += 1) {
    let context = fiber.dependencies?.firstContext;
    for (let contextIndex = 0;context && contextIndex < MAX_CONTEXTS_PER_FIBER; contextIndex += 1) {
      if (isOfficialTooltipProvider(context.memoizedValue))
        return context.memoizedValue;
      context = context.next;
    }
    fiber = fiber.return ?? null;
  }
  return null;
}

// src/runtime/search-button-placement.ts
var TOOLTIP_TRIGGER_STATES = new Set(["closed", "delayed-open", "instant-open"]);
function isSearchTooltipTrigger(element) {
  const state = element.getAttribute("data-state");
  return element.tagName === "SPAN" && state !== null && TOOLTIP_TRIGGER_STATES.has(state);
}
function searchButtonPlacement(search) {
  const parent = search.parentElement;
  if (!parent)
    return null;
  if (isSearchTooltipTrigger(parent) && parent.parentElement) {
    return { parent: parent.parentElement, before: parent };
  }
  return { parent, before: search };
}
function searchTooltipOpen(search) {
  const parent = search.parentElement;
  if (!parent || !isSearchTooltipTrigger(parent))
    return false;
  return parent.getAttribute("data-state") !== "closed" || parent.hasAttribute("aria-describedby");
}

// src/runtime/tooltip-lifecycle.ts
function createTooltipLifecycle(deps) {
  let hovering = false;
  let focused = false;
  let open = false;
  let pending = null;
  let triggerBlocked = false;
  let windowFocused = true;
  let restoredFocusBlocked = false;
  function cancelPending() {
    if (pending === null)
      return;
    deps.cancel(pending);
    pending = null;
  }
  function hide() {
    cancelPending();
    if (open) {
      open = false;
      deps.onClose?.();
    }
    deps.hide();
  }
  function scheduleShow() {
    cancelPending();
    if (triggerBlocked)
      return;
    pending = deps.schedule(() => {
      pending = null;
      if (triggerBlocked || !(hovering || focused) || !deps.canShow())
        return;
      open = true;
      deps.onOpen?.(hide);
      if (!open)
        return;
      deps.show();
    }, deps.resolveDelay?.(deps.delayMs) ?? deps.delayMs);
  }
  return {
    pointerEnter() {
      hovering = true;
      restoredFocusBlocked = false;
      scheduleShow();
    },
    pointerLeave() {
      hovering = false;
      hide();
      if (!focused)
        triggerBlocked = false;
    },
    focus() {
      focused = true;
      if (restoredFocusBlocked)
        return;
      scheduleShow();
    },
    blur() {
      focused = false;
      hide();
      if (windowFocused) {
        restoredFocusBlocked = false;
        if (!hovering)
          triggerBlocked = false;
      }
    },
    windowBlur() {
      if (windowFocused)
        restoredFocusBlocked = focused;
      windowFocused = false;
      focused = false;
      hide();
    },
    windowFocus() {
      windowFocused = true;
    },
    dismiss: hide,
    trigger() {
      triggerBlocked = true;
      hide();
    },
    dispose() {
      hovering = false;
      focused = false;
      triggerBlocked = false;
      windowFocused = true;
      restoredFocusBlocked = false;
      hide();
    }
  };
}

// src/runtime/tooltip-presentation.ts
var OFFICIAL_WINDOW_ZOOM_PROPERTY = "--codex-window-zoom";
function parseOfficialWindowZoom(value) {
  const zoom = Number.parseFloat(value);
  return Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
}
function officialWindowZoom(root) {
  return parseOfficialWindowZoom(window.getComputedStyle(root).getPropertyValue(OFFICIAL_WINDOW_ZOOM_PROPERTY));
}

// src/runtime/_inject.src.ts
var STYLE_ID = "incodex-privacy-style";
var BTN_ATTR = "data-incodex-privacy-toggle";
var TIP_ATTR = "data-incodex-tooltip";
var TIP_HOST_ATTR = "data-incodex-tooltip-host";
var LANDING_ATTR = "data-incodex-landing";
var ERROR_ATTR = "data-incodex-launch-error";
var ERROR_OVERLAY_ATTR = "data-incodex-launch-error-overlay";
var SHORTCUT_LABEL = "⇧⌘N";
var TOOLTIP_FALLBACK_DELAY_MS = 700;
var TOOLTIP_DISMISS_EVENT = "codex:dismiss-tooltips";
var STRIP_CLONE_ATTRS = [
  "id",
  "name",
  "aria-haspopup",
  "aria-expanded",
  "aria-controls",
  "aria-describedby",
  "aria-labelledby",
  "data-state",
  "data-testid",
  "data-test-id",
  "disabled",
  "title",
  "tabindex"
];
var activeTooltipLifecycle = null;
var launchErrorPending = false;
var windowsLaunchErrorHost = null;
function dismissActiveTooltip() {
  activeTooltipLifecycle?.dismiss();
}
function disposeActiveTooltip() {
  activeTooltipLifecycle?.dispose();
  activeTooltipLifecycle = null;
}
var ICON_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M14 18a2 2 0 0 0-4 0"/>
  <path d="m19 11-2.11-6.657a2 2 0 0 0-2.752-1.148l-1.276.61A2 2 0 0 1 12 4H8.5a2 2 0 0 0-1.925 1.456L5 11"/>
  <path d="M2 11h20"/>
  <circle cx="17" cy="18" r="3"/>
  <circle cx="7" cy="18" r="3"/>
</svg>`;
var EXIT_ICON_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="10"/>
  <path d="m15 9-6 6"/>
  <path d="m9 9 6 6"/>
</svg>`;
function isIncognitoWindow() {
  if (typeof window.__incodexIncognito === "boolean")
    return window.__incodexIncognito;
  return false;
}
function isWindowsRenderer() {
  return window.__incodexPlatform === "win32";
}
function shortcutLabel() {
  return isWindowsRenderer() ? "Ctrl+Shift+N" : SHORTCUT_LABEL;
}
function currentLocale() {
  const locale = window.__incodexLocale || document.documentElement.lang || navigator.language || "en";
  return resolveLocale(locale);
}
function t(key) {
  return translate(currentLocale(), key);
}
function labelFor(on) {
  return on ? t("exit") : t("open");
}
function createButtonIcon(source, name, sample) {
  const wrap = document.createElement("span");
  wrap.innerHTML = source.trim();
  const svg = wrap.firstElementChild;
  if (!svg)
    return null;
  svg.setAttribute("data-incodex-icon", name);
  svg.setAttribute("class", sample?.getAttribute("class") || "icon-xs");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("width", sample?.getAttribute("width") || "16");
  svg.setAttribute("height", sample?.getAttribute("height") || "16");
  return svg;
}
function setButtonIcon(btn) {
  const name = isIncognitoWindow() && btn.getAttribute("data-incodex-hovered") === "true" ? "circle-x" : "hat-glasses";
  const current = btn.querySelector("svg[data-incodex-icon]");
  if (current?.getAttribute("data-incodex-icon") === name)
    return;
  const source = name === "circle-x" ? EXIT_ICON_SVG : ICON_SVG;
  const sample = current || btn.querySelector("svg");
  const next = createButtonIcon(source, name, sample);
  if (!next)
    return;
  if (current)
    current.replaceWith(next);
  else if (sample)
    sample.replaceWith(next);
  else
    btn.append(next);
}
function setButtonHover(btn, hovered) {
  btn.setAttribute("data-incodex-hovered", hovered ? "true" : "false");
  setButtonIcon(btn);
}
function apply() {
  const incognito = isIncognitoWindow();
  document.documentElement.setAttribute("data-incodex-window", incognito ? "incognito" : "normal");
  const btn = document.querySelector(`[${BTN_ATTR}]`);
  if (btn) {
    btn.setAttribute("aria-pressed", incognito ? "true" : "false");
    btn.setAttribute("aria-label", labelFor(incognito));
    setButtonIcon(btn);
  }
  const label = document.querySelector("[data-incodex-tooltip-label]");
  if (label)
    label.textContent = labelFor(incognito);
}
function newRequestId() {
  return `incodex-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
async function requestAction(action) {
  if (!window.incodex?.requestIncognitoAction) {
    return { ok: false, reason: "unavailable", code: "UNAVAILABLE" };
  }
  try {
    return await window.incodex.requestIncognitoAction({ action, requestId: newRequestId() }) ?? {
      ok: false,
      reason: "unavailable",
      code: "UNAVAILABLE"
    };
  } catch {
    return { ok: false, reason: "ipc-failed", code: "IPC_FAILED" };
  }
}
async function activate() {
  dismissActiveTooltip();
  if (isIncognitoWindow()) {
    const result2 = await requestAction("quit");
    if (!result2.ok)
      window.close();
    return;
  }
  const result = await requestAction("open");
  if (result.ok) {
    hideLaunchError();
    return;
  }
  showLaunchError();
}
function ensureStyle() {
  let style = document.getElementById(STYLE_ID);
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.append(style);
  }
  style.textContent = `
    [${TIP_HOST_ATTR}] {
      position: fixed;
      z-index: 50;
      display: none;
      pointer-events: none !important;
    }
    [${TIP_HOST_ATTR}][data-open="true"] { display: block; }
    [${TIP_ATTR}] {
      max-width: min(20rem, calc(100vw - 16px));
      pointer-events: none !important;
      user-select: none;
      box-sizing: border-box;
    }
    [${ERROR_OVERLAY_ATTR}] {
      position: fixed;
      top: 16px;
      right: 16px;
      z-index: 60;
      width: min(28rem, calc(100vw - 32px));
    }
  `;
}
var WARNING_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16" class="icon-xs" aria-hidden="true"><path d="M8 9.8a.767.767 0 1 1 0 1.533A.767.767 0 0 1 8 9.8Zm0-5.134c.368 0 .667.299.667.667V8a.667.667 0 0 1-1.334 0V5.333c0-.368.299-.667.667-.667Z"/><path fill-rule="evenodd" d="M8 1.333a6.667 6.667 0 1 1 0 13.334A6.667 6.667 0 0 1 8 1.333Zm0 1.334a5.333 5.333 0 1 0 0 10.666A5.333 5.333 0 0 0 8 2.667Z" clip-rule="evenodd"/></svg>`;
function hideLaunchError() {
  launchErrorPending = false;
  windowsLaunchErrorHost?.remove();
  windowsLaunchErrorHost = null;
  document.querySelector(`[${ERROR_ATTR}]`)?.remove();
}
function showLaunchError() {
  hideLaunchError();
  if (isWindowsRenderer()) {
    launchErrorPending = true;
    ensureLaunchError();
    return;
  }
  const card = document.createElement("div");
  card.setAttribute(ERROR_ATTR, "true");
  card.setAttribute(ERROR_OVERLAY_ATTR, "true");
  card.setAttribute("role", "alert");
  card.className = "alert-root inline-flex flex-col gap-2 rounded-xl px-2 py-2 text-base leading-[1.4] pointer-events-auto box-shadow-lg border border-warning-outline bg-warning-surface text-warning";
  const row = document.createElement("div");
  row.className = "flex min-w-0 items-start gap-1";
  const iconWrap = document.createElement("div");
  iconWrap.className = "flex size-6 shrink-0 grow-0 items-center justify-center self-start";
  iconWrap.innerHTML = WARNING_ICON;
  const mid = document.createElement("div");
  mid.className = "flex min-w-0 flex-1 items-start gap-3";
  const copy = document.createElement("div");
  copy.className = "min-w-0 flex-1 justify-center gap-2 break-words";
  const title = document.createElement("div");
  title.className = "flex min-h-6 items-center text-start font-medium whitespace-pre-wrap";
  title.textContent = t("errorTitle");
  const body = document.createElement("div");
  body.className = "text-start text-warning/80";
  body.textContent = t("errorBody");
  copy.append(title, body);
  const actions = document.createElement("div");
  actions.className = "flex shrink-0 items-center gap-2";
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className = "shrink-0 rounded-full bg-primary-solid px-3 py-1 text-sm font-medium text-primary-solid";
  retry.textContent = t("errorRetry");
  retry.addEventListener("click", () => {
    hideLaunchError();
    activate();
  });
  actions.append(retry);
  mid.append(copy, actions);
  const close = document.createElement("button");
  close.type = "button";
  close.setAttribute("aria-label", t("errorClose"));
  close.className = "flex size-6 shrink-0 grow-0 cursor-interaction items-center justify-center self-start rounded-full hover:bg-background-primary-ghost-hover/5";
  close.innerHTML = CLOSE_SVG;
  close.addEventListener("click", () => hideLaunchError());
  row.append(iconWrap, mid, close);
  card.append(row);
  document.body.append(card);
}
function findSearchButton() {
  return [...document.querySelectorAll("button")].find((btn) => isSearchLabel(btn.getAttribute("aria-label"))) ?? null;
}
function isParkedLeftOfSearch(btn, search) {
  const placement = searchButtonPlacement(search);
  return Boolean(placement && btn.parentElement === placement.parent && btn.nextElementSibling === placement.before);
}
function buttonStillBesideSearch() {
  const btn = document.querySelector(`[${BTN_ATTR}]`);
  const search = findSearchButton();
  return Boolean(btn?.isConnected && search && isParkedLeftOfSearch(btn, search));
}
function injectedTooltipCanShow(btn) {
  const search = findSearchButton();
  return btn.isConnected && (btn.getAttribute("data-incodex-hovered") === "true" || document.activeElement === btn) && !(search && searchTooltipOpen(search));
}
function landingStillMounted() {
  const landing = document.querySelector(`[${LANDING_ATTR}]`);
  if (!isIncognitoWindow() || bannerDismissed())
    return !landing;
  return Boolean(landing);
}
function tooltipMountStillPresent() {
  const host = document.querySelector(`[${TIP_HOST_ATTR}]`);
  const tip = host?.querySelector(`[${TIP_ATTR}]`);
  return Boolean(host?.isConnected && tip?.isConnected && tip.parentElement === host);
}
function needsInject() {
  return !buttonStillBesideSearch() || !tooltipMountStillPresent() || !landingStillMounted() || launchErrorNeedsInject() || profileMaskNeedsInject();
}
function buildButton(search) {
  disposeActiveTooltip();
  const btn = search.cloneNode(false);
  for (const name of STRIP_CLONE_ATTRS)
    btn.removeAttribute(name);
  for (const name of [...btn.attributes].map((attr) => attr.name)) {
    if (name.startsWith("data-") && name !== BTN_ATTR)
      btn.removeAttribute(name);
  }
  btn.setAttribute("type", "button");
  btn.setAttribute(BTN_ATTR, "true");
  btn.setAttribute("data-incodex-hovered", "false");
  btn.className = search.className;
  const svg = createButtonIcon(ICON_SVG, "hat-glasses", search.querySelector("svg"));
  if (svg)
    btn.append(svg);
  const providerTiming = createOfficialTooltipTimingBridge(findSearchButton);
  const tooltipLifecycle = createTooltipLifecycle({
    delayMs: TOOLTIP_FALLBACK_DELAY_MS,
    resolveDelay: providerTiming.resolveDelay,
    schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
    cancel: (id) => window.clearTimeout(id),
    canShow: () => injectedTooltipCanShow(btn),
    onOpen: providerTiming.activate,
    onClose: providerTiming.deactivate,
    show: () => showTooltip(btn),
    hide: hideTooltip
  });
  activeTooltipLifecycle = tooltipLifecycle;
  btn.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
    setButtonHover(btn, false);
    tooltipLifecycle.trigger();
    activate();
  }, true);
  btn.addEventListener("pointerenter", () => {
    setButtonHover(btn, true);
    tooltipLifecycle.pointerEnter();
  });
  btn.addEventListener("pointerleave", () => {
    setButtonHover(btn, false);
    tooltipLifecycle.pointerLeave();
  });
  btn.addEventListener("focus", tooltipLifecycle.focus);
  btn.addEventListener("blur", tooltipLifecycle.blur);
  return btn;
}
function createTooltipElement() {
  const tip = document.createElement("div");
  tip.setAttribute(TIP_ATTR, "true");
  tip.setAttribute("role", "tooltip");
  tip.className = "z-50 w-fit select-none text-sm whitespace-normal break-words rounded-lg border border-text bg-primary-solid text-primary-solid px-2 py-1.5";
  const text = document.createElement("div");
  text.className = "flex items-center gap-2";
  const label = document.createElement("div");
  label.className = "min-w-0";
  label.setAttribute("data-incodex-tooltip-label", "true");
  const kbd = document.createElement("kbd");
  kbd.className = "inline-flex !rounded-md !border-0 !bg-current/10 !font-sans !text-xs !text-current !shadow-none !px-1.5 !py-0.5 !leading-none";
  kbd.textContent = shortcutLabel();
  text.append(label, kbd);
  tip.append(text);
  return tip;
}
function ensureTooltipMount() {
  let host = document.querySelector(`[${TIP_HOST_ATTR}]`);
  if (!host) {
    host = document.createElement("div");
    host.setAttribute(TIP_HOST_ATTR, "true");
    document.body.append(host);
  }
  let tip = host.querySelector(`[${TIP_ATTR}]`);
  if (!tip) {
    tip = document.querySelector(`[${TIP_ATTR}]`) ?? createTooltipElement();
    if (tip.parentElement !== host)
      host.append(tip);
  }
  return tip;
}
function tooltipEl() {
  return ensureTooltipMount();
}
var TOOLTIP_SIDE_OFFSET = 2;
function showTooltip(btn) {
  const tip = tooltipEl();
  const host = tip.parentElement;
  if (!host)
    return;
  const label = tip.querySelector("[data-incodex-tooltip-label]");
  if (label)
    label.textContent = labelFor(btn.getAttribute("aria-pressed") === "true");
  const zoom = officialWindowZoom(document.documentElement);
  tip.style.zoom = zoom === 1 ? "" : String(zoom);
  host.style.visibility = "hidden";
  host.setAttribute("data-open", "true");
  const rect = btn.getBoundingClientRect();
  const tipRect = tip.getBoundingClientRect();
  const left = Math.min(window.innerWidth - tipRect.width - 8, Math.max(8, rect.left + rect.width / 2 - tipRect.width / 2));
  host.style.left = `${left}px`;
  host.style.top = "auto";
  host.style.bottom = `${Math.max(8, window.innerHeight - rect.top + TOOLTIP_SIDE_OFFSET)}px`;
  host.style.visibility = "";
}
function hideTooltip() {
  const host = document.querySelector(`[${TIP_HOST_ATTR}]`);
  if (!host)
    return;
  host.removeAttribute("data-open");
  host.style.bottom = "";
  host.style.left = "";
  host.style.top = "";
}
var BANNER_DISMISS_KEY = "incodex-banner-dismissed";
var BANNER_HOST_ATTR = "data-incodex-banner-host";
var BANNER_TITLE_ATTR = "data-incodex-banner-title";
var BANNER_BODY_ATTR = "data-incodex-banner-body";
var CLOSE_SVG = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg" class="icon-xs" aria-hidden="true"><path d="M4.2 4.2l7.6 7.6M11.8 4.2l-7.6 7.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;
function bannerDismissed() {
  try {
    return window.sessionStorage.getItem(BANNER_DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}
function refreshUiProbe() {
  const incognito = isIncognitoWindow();
  window.__incodexProfileMaskHealth = profileMaskHealth();
  window.__incodexUiProbe = deriveUiProbe({
    incognito,
    buttonPresent: buttonStillBesideSearch(),
    tooltipPresent: tooltipMountStillPresent(),
    bannerPresent: Boolean(document.querySelector(`[${BANNER_HOST_ATTR}]`)?.querySelector(`[${LANDING_ATTR}]`)),
    bannerDismissed: incognito && bannerDismissed()
  });
}
function dismissBanner() {
  try {
    window.sessionStorage.setItem(BANNER_DISMISS_KEY, "1");
  } catch {}
  document.querySelector(`[${LANDING_ATTR}]`)?.closest(`[${BANNER_HOST_ATTR}]`)?.remove();
  document.querySelector(`[${LANDING_ATTR}]`)?.remove();
  refreshUiProbe();
}
function classNameOf(element) {
  return element.getAttribute("class") ?? "";
}
function findOfficialBannerSlot() {
  return [...document.querySelectorAll("div")].find((el) => {
    if (el.hasAttribute(BANNER_HOST_ATTR))
      return false;
    return classNameOf(el).split(/\s+/).includes("home-banners");
  }) ?? null;
}
function mountInOfficialBannerSlot(element) {
  const slot = findOfficialBannerSlot();
  if (!slot)
    return false;
  if (slot.firstElementChild !== element)
    slot.insertBefore(element, slot.firstChild);
  return true;
}
function cloneOfficialPrimaryAction() {
  const slot = findOfficialBannerSlot();
  const source = [...slot?.querySelectorAll("button") ?? []].find((button) => button.textContent?.trim() && !button.closest(`[${BANNER_HOST_ATTR}]`) && !button.closest(`[${ERROR_ATTR}]`)) ?? document.querySelector("button.bg-primary-solid");
  if (!source)
    return null;
  const clone = source.cloneNode(false);
  for (const name of STRIP_CLONE_ATTRS)
    clone.removeAttribute(name);
  for (const name of [...clone.attributes].map((attribute) => attribute.name)) {
    if (name.startsWith("data-"))
      clone.removeAttribute(name);
  }
  clone.type = "button";
  clone.disabled = false;
  return clone;
}
function buildOfficialHomeBanner(options) {
  const host = document.createElement("div");
  host.setAttribute(options.hostAttribute, "true");
  const card = document.createElement("aside");
  if (options.cardAttribute)
    card.setAttribute(options.cardAttribute, "true");
  card.setAttribute("aria-live", options.warning ? "assertive" : "polite");
  if (options.warning)
    card.setAttribute("role", "alert");
  card.className = `relative isolate flex w-full items-center gap-4 overflow-hidden rounded-2xl border bg-surface py-2 ps-3 pe-2 text-sm text-default shadow-xs lg:mx-auto electron:border-0 electron:ring-[0.5px] electron:ring-border-strong ${options.warning ? "border-text-warning/30" : "border-primary-outline"}`;
  const wash = document.createElement("div");
  wash.setAttribute("aria-hidden", "true");
  wash.className = `absolute inset-0 -z-10 ${options.warning ? "bg-background-warning-surface/30" : "bg-primary-soft"}`;
  const row = document.createElement("div");
  row.className = "flex h-full w-full min-w-0 items-center gap-2";
  const visual = document.createElement("div");
  visual.className = `flex size-12 shrink-0 items-center justify-center self-center ${options.warning ? "text-warning" : "text-secondary"}`;
  visual.innerHTML = options.icon.trim();
  const svg = visual.querySelector("svg");
  if (svg) {
    svg.setAttribute("class", "icon-sm");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", String(options.iconSize));
    svg.setAttribute("height", String(options.iconSize));
  }
  const copy = document.createElement("div");
  copy.className = "min-w-0 flex-1";
  const titleWrap = document.createElement("div");
  titleWrap.className = "flex flex-wrap items-center gap-2";
  const title = document.createElement("div");
  title.className = "min-w-0 text-base font-medium text-default";
  title.setAttribute(BANNER_TITLE_ATTR, "true");
  title.textContent = options.title;
  titleWrap.append(title);
  const body = document.createElement("div");
  body.className = "text-sm leading-tight text-pretty text-secondary";
  body.setAttribute(BANNER_BODY_ATTR, "true");
  body.textContent = options.body;
  copy.append(titleWrap, body);
  const actions = document.createElement("div");
  actions.className = "flex items-center gap-2 self-center max-[400px]:w-full max-[400px]:justify-center max-[400px]:self-stretch";
  if (options.primaryAction) {
    const primary = cloneOfficialPrimaryAction() ?? document.createElement("button");
    primary.type = "button";
    if (!primary.className) {
      primary.className = "shrink-0 rounded-full bg-primary-solid px-3 py-1 text-sm font-medium text-primary-solid";
    }
    primary.textContent = options.primaryAction.label;
    primary.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      options.primaryAction?.onClick();
    });
    actions.append(primary);
  }
  const close = document.createElement("button");
  close.type = "button";
  close.setAttribute("aria-label", options.closeLabel);
  close.className = "flex size-8 shrink-0 items-center justify-center rounded-lg border-transparent text-codex-description hover:text-default";
  close.innerHTML = CLOSE_SVG;
  close.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    options.onClose();
  }, true);
  actions.append(close);
  row.append(visual, copy, actions);
  card.append(wash, row);
  host.append(card);
  return host;
}
function buildLanding() {
  return buildOfficialHomeBanner({
    body: t("body"),
    cardAttribute: LANDING_ATTR,
    closeLabel: t("dismiss"),
    hostAttribute: BANNER_HOST_ATTR,
    icon: ICON_SVG,
    iconSize: 24,
    onClose: dismissBanner,
    title: t("title")
  });
}
function buildWindowsLaunchErrorBanner() {
  return buildOfficialHomeBanner({
    body: t("errorBody"),
    closeLabel: t("errorClose"),
    hostAttribute: ERROR_ATTR,
    icon: WARNING_ICON,
    iconSize: 20,
    onClose: hideLaunchError,
    primaryAction: {
      label: t("errorRetry"),
      onClick: () => {
        hideLaunchError();
        activate();
      }
    },
    title: t("errorTitle"),
    warning: true
  });
}
function launchErrorNeedsInject() {
  if (!isWindowsRenderer() || !launchErrorPending)
    return false;
  const slot = findOfficialBannerSlot();
  return !slot || !windowsLaunchErrorHost?.isConnected || windowsLaunchErrorHost.parentElement !== slot;
}
function ensureLaunchError() {
  if (!isWindowsRenderer() || !launchErrorPending)
    return;
  if (!windowsLaunchErrorHost)
    windowsLaunchErrorHost = buildWindowsLaunchErrorBanner();
  windowsLaunchErrorHost.className = "";
  mountInOfficialBannerSlot(windowsLaunchErrorHost);
}
function syncLandingCopy(host) {
  const title = host.querySelector(`[${BANNER_TITLE_ATTR}]`);
  const body = host.querySelector(`[${BANNER_BODY_ATTR}]`);
  const close = host.querySelector("button[aria-label]");
  if (title)
    title.textContent = t("title");
  if (body)
    body.textContent = t("body");
  if (close)
    close.setAttribute("aria-label", t("dismiss"));
}
function removeLanding() {
  document.querySelector(`[${BANNER_HOST_ATTR}]`)?.remove();
  document.querySelector(`[${LANDING_ATTR}]`)?.remove();
}
function ensureLanding() {
  if (!isIncognitoWindow() || bannerDismissed()) {
    removeLanding();
    return;
  }
  let host = document.querySelector(`[${BANNER_HOST_ATTR}]`);
  if (!host)
    host = buildLanding();
  syncLandingCopy(host);
  host.className = "";
  mountInOfficialBannerSlot(host);
}
function ensureButton() {
  let btn = document.querySelector(`[${BTN_ATTR}]`);
  const search = findSearchButton();
  const placement = search ? searchButtonPlacement(search) : null;
  if (!search || !placement) {
    if (btn?.isConnected)
      dismissActiveTooltip();
    else
      disposeActiveTooltip();
    return;
  }
  if (!btn)
    btn = buildButton(search);
  if (!isParkedLeftOfSearch(btn, search)) {
    placement.parent.insertBefore(btn, placement.before);
  }
  apply();
  ensureTooltipMount();
}
function onKeydown(event) {
  if (event.key === "Escape") {
    dismissActiveTooltip();
    return;
  }
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey)
    return;
  if (event.code !== "KeyN" && event.key.toLowerCase() !== "n")
    return;
  event.preventDefault();
  event.stopImmediatePropagation();
  activate();
}
var PROFILE_OBSERVED_ATTRIBUTES = [
  "aria-controls",
  "class",
  "src",
  "style",
  "data-incodex-profile-mask",
  "data-incodex-profile-mask-name",
  "data-incodex-profile-mask-avatar"
];
function observerOptions() {
  const options = { childList: true, subtree: true };
  if (profileObservationRequired()) {
    options.attributes = true;
    options.characterData = true;
    options.attributeFilter = PROFILE_OBSERVED_ATTRIBUTES;
  }
  return options;
}
function profileObservationRequired() {
  return isIncognitoWindow() && window.__incodexProfileMask !== null && window.__incodexProfileMask !== undefined;
}
function createMutationObserver() {
  let scheduled = false;
  return new MutationObserver(function handleMutation() {
    if (!needsInject() || scheduled)
      return;
    scheduled = true;
    requestAnimationFrame(function injectOnAnimationFrame() {
      scheduled = false;
      if (!needsInject())
        return;
      ensureButton();
      ensureLanding();
      ensureLaunchError();
      ensureProfileMask();
      refreshUiProbe();
    });
  });
}
function ensureMutationObserver() {
  const profileRequired = profileObservationRequired();
  let observer = window.__incodexMutationObserver;
  if (!observer) {
    observer = createMutationObserver();
    window.__incodexMutationObserver = observer;
  }
  observer.observe(document.documentElement, observerOptions());
  window.__incodexProfileObservationEnabled = profileRequired;
}
function start() {
  if (window.__incodexStarted) {
    ensureStyle();
    ensureButton();
    ensureLanding();
    ensureLaunchError();
    ensureProfileMask();
    refreshUiProbe();
    ensureMutationObserver();
    return;
  }
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply();
  ensureLanding();
  ensureLaunchError();
  ensureProfileMask();
  refreshUiProbe();
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("blur", () => activeTooltipLifecycle?.windowBlur());
  window.addEventListener("focus", () => activeTooltipLifecycle?.windowFocus());
  window.addEventListener(TOOLTIP_DISMISS_EVENT, () => activeTooltipLifecycle?.dismiss());
  ensureMutationObserver();
}
window.__incodexRefreshProfileMaskHealth = profileMaskHealth;
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}
