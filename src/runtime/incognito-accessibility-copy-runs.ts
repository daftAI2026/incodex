// Semantic drag-instruction spans are authored per locale. Do not infer these
// spans by searching for English words or by splitting the translated sentence
// at runtime: inflection and natural word order differ across the catalog.
export type DragInstructionRunRole = "primary" | "secondary";

export type DragInstructionRun = {
  readonly text: string;
  readonly role: DragInstructionRunRole;
};

const secondary = (text: string): DragInstructionRun => ({ text, role: "secondary" });
const primary = (text: string): DragInstructionRun => ({ text, role: "primary" });

const authoredRuns = (...runs: DragInstructionRun[]): string => JSON.stringify(runs);

export const ACCESSIBILITY_DRAG_INSTRUCTION_RUNS: Readonly<Record<string, string>> = /* @__PURE__ */ (() => ({
  am: authoredRuns(
    primary("የተደራሽነት መዳረሻ"),
    secondary(" ለመፍቀድ "),
    primary("ChatGPT"),
    secondary("ን ወደ ከላይ ያለው ዝርዝር ይጎትቱ"),
  ),
  ar: authoredRuns(
    secondary("اسحب "),
    primary("ChatGPT"),
    secondary(" إلى القائمة أعلاه لمنحه إذن "),
    primary("تسهيلات الاستخدام"),
  ),
  "bg-BG": authoredRuns(
    secondary("Плъзнете "),
    primary("ChatGPT"),
    secondary(" в списъка по-горе, за да разрешите "),
    primary("Улеснен достъп"),
  ),
  "bn-BD": authoredRuns(
    primary("অ্যাক্সেসিবিলিটি"),
    secondary(" অনুমতি দিতে "),
    primary("ChatGPT"),
    secondary("-কে উপরের তালিকায় টেনে আনুন"),
  ),
  "bs-BA": authoredRuns(
    secondary("Prevucite "),
    primary("ChatGPT"),
    secondary(" na gornju listu da biste dozvolili "),
    primary("pristupačnost"),
  ),
  "ca-ES": authoredRuns(
    secondary("Arrossega "),
    primary("ChatGPT"),
    secondary(" a la llista de dalt per permetre l’"),
    primary("accessibilitat"),
  ),
  "cs-CZ": authoredRuns(
    secondary("Přetáhněte "),
    primary("ChatGPT"),
    secondary(" do seznamu výše a povolte "),
    primary("Zpřístupnění"),
  ),
  "da-DK": authoredRuns(
    secondary("Træk "),
    primary("ChatGPT"),
    secondary(" til listen ovenfor for at tillade "),
    primary("Tilgængelighed"),
  ),
  "de-DE": authoredRuns(
    secondary("Ziehe "),
    primary("ChatGPT"),
    secondary(" in die Liste oben, um den Zugriff auf die "),
    primary("Bedienungshilfen"),
    secondary(" zu erlauben"),
  ),
  "el-GR": authoredRuns(
    secondary("Σύρετε το "),
    primary("ChatGPT"),
    secondary(" στην παραπάνω λίστα για να επιτρέψετε την "),
    primary("Προσβασιμότητα"),
  ),
  "es-419": authoredRuns(
    secondary("Arrastra "),
    primary("ChatGPT"),
    secondary(" a la lista de arriba para permitir la "),
    primary("Accesibilidad"),
  ),
  "es-ES": authoredRuns(
    secondary("Arrastra "),
    primary("ChatGPT"),
    secondary(" a la lista de arriba para permitir la "),
    primary("Accesibilidad"),
  ),
  "et-EE": authoredRuns(
    primary("Juurdepääsetavuse"),
    secondary(" lubamiseks lohista "),
    primary("ChatGPT"),
    secondary(" ülalolevasse loendisse"),
  ),
  fa: authoredRuns(
    secondary("برای اجازه دادن به "),
    primary("دسترسی‌پذیری"),
    secondary("، "),
    primary("ChatGPT"),
    secondary(" را به فهرست بالا بکشید"),
  ),
  "fi-FI": authoredRuns(
    secondary("Salli "),
    primary("Käyttöapu"),
    secondary(" vetämällä "),
    primary("ChatGPT"),
    secondary(" yllä olevaan luetteloon"),
  ),
  "fr-CA": authoredRuns(
    secondary("Faites glisser "),
    primary("ChatGPT"),
    secondary(" dans la liste ci-dessus pour autoriser l’"),
    primary("accessibilité"),
  ),
  "fr-FR": authoredRuns(
    secondary("Faites glisser "),
    primary("ChatGPT"),
    secondary(" dans la liste ci-dessus pour autoriser l’"),
    primary("accessibilité"),
  ),
  "gu-IN": authoredRuns(
    primary("ઍક્સેસિબિલિટી"),
    secondary(" મંજૂર કરવા "),
    primary("ChatGPT"),
    secondary("ને ઉપરની સૂચિમાં ખેંચો"),
  ),
  "hi-IN": authoredRuns(
    primary("ऐक्सेसिबिलिटी"),
    secondary(" की अनुमति देने के लिए "),
    primary("ChatGPT"),
    secondary(" को ऊपर दी गई सूची में खींचें"),
  ),
  "hr-HR": authoredRuns(
    secondary("Povucite "),
    primary("ChatGPT"),
    secondary(" na gornji popis da biste dopustili "),
    primary("pristupačnost"),
  ),
  "hu-HU": authoredRuns(
    secondary("Húzza a "),
    primary("ChatGPT"),
    secondary("-t a fenti listára a "),
    primary("Kisegítő lehetőségek"),
    secondary(" engedélyezéséhez"),
  ),
  "hy-AM": authoredRuns(
    primary("Մատչելիությունը"),
    secondary(" թույլատրելու համար "),
    primary("ChatGPT"),
    secondary("-ը քաշեք վերևի ցանկ"),
  ),
  "id-ID": authoredRuns(
    secondary("Seret "),
    primary("ChatGPT"),
    secondary(" ke daftar di atas untuk mengizinkan "),
    primary("Aksesibilitas"),
  ),
  "is-IS": authoredRuns(
    secondary("Dragðu "),
    primary("ChatGPT"),
    secondary(" á listann hér að ofan til að leyfa "),
    primary("aðgengi"),
  ),
  "it-IT": authoredRuns(
    secondary("Trascina "),
    primary("ChatGPT"),
    secondary(" nell’elenco qui sopra per consentire l’accesso all’"),
    primary("Accessibilità"),
  ),
  "ja-JP": authoredRuns(
    primary("アクセシビリティ"),
    secondary("を許可するには、"),
    primary("ChatGPT"),
    secondary(" を上のリストにドラッグします"),
  ),
  "ka-GE": authoredRuns(
    primary("წვდომადობის"),
    secondary(" დასაშვებად "),
    primary("ChatGPT"),
    secondary(" ზემოთ მოცემულ სიაში გადაათრიეთ"),
  ),
  kk: authoredRuns(
    primary("Арнайы мүмкіндіктерге"),
    secondary(" рұқсат беру үшін "),
    primary("ChatGPT"),
    secondary("-ті жоғарыдағы тізімге сүйреп апарыңыз"),
  ),
  "kn-IN": authoredRuns(
    primary("ಆಕ್ಸೆಸಿಬಿಲಿಟಿಗೆ"),
    secondary(" ಅನುಮತಿಸಲು "),
    primary("ChatGPT"),
    secondary(" ಅನ್ನು ಮೇಲಿನ ಪಟ್ಟಿಗೆ ಎಳೆಯಿರಿ"),
  ),
  "ko-KR": authoredRuns(
    primary("손쉬운 사용"),
    secondary("을 허용하려면 "),
    primary("ChatGPT"),
    secondary("를 위 목록으로 드래그하세요"),
  ),
  lt: authoredRuns(
    secondary("Nuvilkite "),
    primary("ChatGPT"),
    secondary(" į aukščiau esantį sąrašą, kad leistumėte "),
    primary("Prieinamumą"),
  ),
  "lv-LV": authoredRuns(
    secondary("Velciet "),
    primary("ChatGPT"),
    secondary(" uz iepriekš redzamo sarakstu, lai atļautu "),
    primary("pieejamību"),
  ),
  "mk-MK": authoredRuns(
    secondary("Повлечете го "),
    primary("ChatGPT"),
    secondary(" во списокот погоре за да дозволите "),
    primary("Пристапност"),
  ),
  ml: authoredRuns(
    primary("ആക്‌സസിബിലിറ്റി"),
    secondary(" അനുവദിക്കാൻ "),
    primary("ChatGPT"),
    secondary(" മുകളിലുള്ള പട്ടികയിലേക്ക് വലിച്ചിടുക"),
  ),
  mn: authoredRuns(
    primary("Хандалт"),
    secondary(" зөвшөөрөхийн тулд "),
    primary("ChatGPT"),
    secondary("-г дээрх жагсаалт руу чирнэ үү"),
  ),
  "mr-IN": authoredRuns(
    primary("सुलभतेला"),
    secondary(" परवानगी देण्यासाठी "),
    primary("ChatGPT"),
    secondary(" वरील सूचीमध्ये ड्रॅग करा"),
  ),
  "ms-MY": authoredRuns(
    secondary("Seret "),
    primary("ChatGPT"),
    secondary(" ke senarai di atas untuk membenarkan "),
    primary("Kebolehcapaian"),
  ),
  "my-MM": authoredRuns(
    primary("အသုံးပြုနိုင်မှုကို"),
    secondary(" ခွင့်ပြုရန် "),
    primary("ChatGPT"),
    secondary(" ကို အပေါ်ရှိစာရင်းထဲသို့ ဆွဲချပါ"),
  ),
  "nb-NO": authoredRuns(
    secondary("Dra "),
    primary("ChatGPT"),
    secondary(" til listen ovenfor for å tillate "),
    primary("Tilgjengelighet"),
  ),
  "nl-NL": authoredRuns(
    secondary("Sleep "),
    primary("ChatGPT"),
    secondary(" naar de bovenstaande lijst om "),
    primary("Toegankelijkheid"),
    secondary(" toe te staan"),
  ),
  pa: authoredRuns(
    primary("ਪਹੁੰਚਯੋਗਤਾ"),
    secondary(" ਦੀ ਮਨਜ਼ੂਰੀ ਦੇਣ ਲਈ "),
    primary("ChatGPT"),
    secondary(" ਨੂੰ ਉੱਪਰ ਦਿੱਤੀ ਸੂਚੀ ਵਿੱਚ ਖਿੱਚੋ"),
  ),
  "pl-PL": authoredRuns(
    secondary("Przeciągnij "),
    primary("ChatGPT"),
    secondary(" na powyższą listę, aby zezwolić na "),
    primary("Dostępność"),
  ),
  "pt-BR": authoredRuns(
    secondary("Arraste o "),
    primary("ChatGPT"),
    secondary(" para a lista acima para permitir a "),
    primary("Acessibilidade"),
  ),
  "pt-PT": authoredRuns(
    secondary("Arraste o "),
    primary("ChatGPT"),
    secondary(" para a lista acima para permitir a "),
    primary("Acessibilidade"),
  ),
  "ro-RO": authoredRuns(
    secondary("Trageți "),
    primary("ChatGPT"),
    secondary(" în lista de mai sus pentru a permite "),
    primary("Accesibilitatea"),
  ),
  "ru-RU": authoredRuns(
    secondary("Перетащите "),
    primary("ChatGPT"),
    secondary(" в список выше, чтобы разрешить «"),
    primary("Универсальный доступ"),
    secondary("»"),
  ),
  "sk-SK": authoredRuns(
    secondary("Presuňte "),
    primary("ChatGPT"),
    secondary(" do zoznamu vyššie a povoľte "),
    primary("Prístupnosť"),
  ),
  "sl-SI": authoredRuns(
    secondary("Povlecite "),
    primary("ChatGPT"),
    secondary(" na zgornji seznam, da dovolite "),
    primary("Dostopnost"),
  ),
  "so-SO": authoredRuns(
    primary("ChatGPT"),
    secondary(" u jiid liiska kore si aad u oggolaato "),
    primary("Helitaanka"),
  ),
  "sq-AL": authoredRuns(
    secondary("Tërhiq "),
    primary("ChatGPT"),
    secondary(" në listën më sipër për të lejuar "),
    primary("Aksesueshmërinë"),
  ),
  "sr-RS": authoredRuns(
    secondary("Превуците "),
    primary("ChatGPT"),
    secondary(" на листу изнад да бисте дозволили "),
    primary("Приступачност"),
  ),
  "sv-SE": authoredRuns(
    secondary("Dra "),
    primary("ChatGPT"),
    secondary(" till listan ovan för att tillåta "),
    primary("Hjälpmedel"),
  ),
  "sw-TZ": authoredRuns(
    secondary("Buruta "),
    primary("ChatGPT"),
    secondary(" kwenye orodha iliyo hapo juu ili kuruhusu "),
    primary("Ufikivu"),
  ),
  "ta-IN": authoredRuns(
    primary("அணுகல்தன்மையை"),
    secondary(" அனுமதிக்க "),
    primary("ChatGPT"),
    secondary("-ஐ மேலே உள்ள பட்டியலுக்கு இழுக்கவும்"),
  ),
  "te-IN": authoredRuns(
    primary("యాక్సెసిబిలిటీని"),
    secondary(" అనుమతించడానికి "),
    primary("ChatGPT"),
    secondary("ను పై జాబితాలోకి లాగండి"),
  ),
  "th-TH": authoredRuns(
    secondary("ลาก "),
    primary("ChatGPT"),
    secondary(" ไปยังรายการด้านบนเพื่ออนุญาต"),
    primary("การช่วยการเข้าถึง"),
  ),
  tl: authoredRuns(
    secondary("I-drag ang "),
    primary("ChatGPT"),
    secondary(" sa listahan sa itaas para payagan ang "),
    primary("Accessibility"),
  ),
  "tr-TR": authoredRuns(
    primary("Erişilebilirliğe"),
    secondary(" izin vermek için "),
    primary("ChatGPT"),
    secondary("'yi yukarıdaki listeye sürükleyin"),
  ),
  "uk-UA": authoredRuns(
    secondary("Перетягніть "),
    primary("ChatGPT"),
    secondary(" до списку «"),
    primary("Доступність"),
    secondary("» вище, щоб надати дозвіл"),
  ),
  ur: authoredRuns(
    primary("قابلِ رسائی"),
    secondary(" کی اجازت دینے کے لیے "),
    primary("ChatGPT"),
    secondary(" کو اوپر موجود فہرست میں گھسیٹیں"),
  ),
  "vi-VN": authoredRuns(
    secondary("Kéo "),
    primary("ChatGPT"),
    secondary(" vào danh sách ở trên để cho phép "),
    primary("Trợ năng"),
  ),
  en: authoredRuns(
    secondary("Drag "),
    primary("ChatGPT"),
    secondary(" to the list above to allow "),
    primary("Accessibility"),
  ),
  "zh-CN": authoredRuns(
    secondary("将 "),
    primary("ChatGPT"),
    secondary(" 拖到上方的“"),
    primary("无障碍"),
    secondary("”列表中，然后开启对应权限"),
  ),
  "zh-HK": authoredRuns(
    secondary("將 "),
    primary("ChatGPT"),
    secondary(" 拖到上方的「"),
    primary("輔助使用"),
    secondary("」列表中，然後啟用權限"),
  ),
  "zh-TW": authoredRuns(
    secondary("將 "),
    primary("ChatGPT"),
    secondary(" 拖曳到上方的「"),
    primary("輔助使用"),
    secondary("」列表中，然後啟用權限"),
  ),
}))();

export type AccessibilityCopyWithDragRuns<T extends Record<string, { dragInstruction: string }>> = {
  [K in keyof T]: T[K] & { dragInstructionRuns: string };
};

export function attachAccessibilityDragInstructionRuns<T extends Record<string, { dragInstruction: string }>>(
  copy: T,
): AccessibilityCopyWithDragRuns<T> {
  return Object.fromEntries(
    Object.entries(copy).map(([locale, value]) => {
      // Styling is optional presentation metadata. A future untranslated
      // span must not throw while initializing the entire Runtime catalog.
      const dragInstructionRuns = ACCESSIBILITY_DRAG_INSTRUCTION_RUNS[locale] ?? "";
      return [locale, { ...value, dragInstructionRuns }];
    }),
  ) as AccessibilityCopyWithDragRuns<T>;
}
