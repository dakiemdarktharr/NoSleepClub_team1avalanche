(() => {
  'use strict';
  const workspace = document.getElementById('workspace');
  const toast = document.getElementById('toast');
  let toastTimer, recordingSession = null, audio, audioUrl, installEvent;
  let reportId = '', reportDigest = '', preparedTransaction = null, walletAccount = '', confirmedReceipt = false, pollSignature = null, polling = false, busy = false;
  let navigationSequence = 0, userNavigationPending = 0, navigationController = null;
  const initialAsOf = document.getElementById('as-of')?.value || '';
  let renderedAsOf = initialAsOf;
  // These drafts live only in this tab's memory and are not sent until submitted.
  const investigationDrafts = new Map();
  const formDrafts = new Map(), detailDrafts = new Map();
  const formKey = (p, form) => [p.view,p.selected,form.dataset.form,form.querySelector('[name="entry_id"],[name="run_id"],[type="hidden"][name="confirmed"]')?.value || ''].join('|');
  const rememberForms = p => {
    workspace.querySelectorAll('.review-form,.feedback-form').forEach(form => {
      formDrafts.set(formKey(p,form), [...form.elements].filter(e => e.name && e.type !== 'hidden').map(e => ({name:e.name,value:e.value,checked:e.checked})));
    });
    detailDrafts.set(p.view+'|'+p.selected,[...workspace.querySelectorAll('details[open]')].map(e=>e.querySelector('summary')?.textContent));
    while(formDrafts.size>100) formDrafts.delete(formDrafts.keys().next().value);
    while(detailDrafts.size>100) detailDrafts.delete(detailDrafts.keys().next().value);
  };
  const restoreForms = p => {
    workspace.querySelectorAll('.review-form,.feedback-form').forEach(form => {
      for (const saved of formDrafts.get(formKey(p,form)) || []) { const field=form.elements.namedItem(saved.name); if(field) { field.value=saved.value; if(field.type==='checkbox') field.checked=saved.checked; } }
    });
    const open=detailDrafts.get(p.view+'|'+p.selected)||[];
    workspace.querySelectorAll('details').forEach(d=>{d.open=open.includes(d.querySelector('summary')?.textContent);});
  };
  let comparisonDraft = null;
  const notify = (message, error = false) => {
    toast.textContent = message;
    toast.classList.toggle('error', error);
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toast.hidden = true; }, 6000);
  };
  const current = () => {
    const p = new URLSearchParams(location.search);
    return { view: p.get('view') || 'overview', selected: p.get('selected') || '', ids: p.get('ids') || '', q: p.get('q') || '', as_of: p.get('as_of') || '', page:p.get('page') || '', snapshot:p.get('snapshot') || '' };
  };
  let renderedRoute = current();
  const setActive = view => document.querySelectorAll('.nav-link').forEach(a => {
    const active = a.dataset.view === view;
    a.classList.toggle('active', active);
    if (active) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
  });
  async function navigate(view, options = {}, push = true, background = false) {
    if (background && userNavigationPending) return false;
    const sequence = ++navigationSequence;
    if (!background) userNavigationPending = sequence;
    navigationController?.abort();
    const controller = new AbortController(); navigationController = controller;
    const params = new URLSearchParams({ view });
    for (const [key, value] of Object.entries(options)) if (value && key !== 'view') params.set(key, value);
    if (!params.has('as_of') && asOf()) params.set('as_of', asOf());
    try {
    const response = await fetch('/ui?' + params, { headers: { Accept: 'text/html' }, cache: 'no-store', signal:controller.signal });
    if (!response.ok) throw new Error('Không tải được màn hình. Vui lòng thử lại.');
    const markup = await response.text();
    if (sequence !== navigationSequence) return false;
    const previous = renderedRoute;
    rememberForms(previous);
    const sameWorkspace = previous.view === view && previous.selected === (options.selected || '');
    const draft = document.getElementById('chat-input')?.value || '';
    const oldInvestigation = document.getElementById('investigate-query');
    if (oldInvestigation) investigationDrafts.set(previous.selected, oldInvestigation.value);
    const investigationDraft = investigationDrafts.get(options.selected || '') || '';
    const oldComparison = workspace.querySelector('form[data-form="compare"]');
    if (oldComparison) comparisonDraft = [...oldComparison.querySelectorAll('input[name="ids"]:checked')].map(input => input.value);
    const focused = workspace.contains(document.activeElement) ? document.activeElement : null;
    const focusedId = sameWorkspace ? focused?.id : '';
    const focusedAction = sameWorkspace ? focused?.dataset?.action : '';
    const focusedDataId = sameWorkspace ? focused?.dataset?.id : '';
    const focusedDecision = sameWorkspace ? focused?.dataset?.decision : '';
    const selection = focused && typeof focused.selectionStart === 'number' ? [focused.selectionStart, focused.selectionEnd] : null;
    const dockOpen = document.getElementById('mira-dock')?.classList.contains('open');
    workspace.innerHTML = markup;
    restoreForms({view,selected:options.selected || ''});
    const nextInput = document.getElementById('chat-input'); if (nextInput) nextInput.value = draft;
    const nextInvestigation = document.getElementById('investigate-query'); if (nextInvestigation) nextInvestigation.value = investigationDraft;
    if (comparisonDraft) workspace.querySelectorAll('form[data-form="compare"] input[name="ids"]').forEach(input => { input.checked = comparisonDraft.includes(input.value); });
    if (dockOpen) document.getElementById('mira-dock')?.classList.add('open');
    if (push) history.pushState({}, '', '/?' + params);
    renderedAsOf = params.get('as_of') || initialAsOf;
    renderedRoute = {view, ...options, as_of:renderedAsOf}; renderedRoute.view=view;
    setActive(view);
    document.getElementById('navigation')?.classList.remove('open');
    document.querySelector('.mobile-menu')?.setAttribute('aria-expanded', 'false');
    refreshInstall();
    updateRecordingUi();
    const nextFocus = focusedId ? document.getElementById(focusedId) : (focusedAction ? [...workspace.querySelectorAll('[data-action]')].find(element => element.dataset.action === focusedAction && element.dataset.id === focusedDataId && element.dataset.decision === focusedDecision) : null);
    if (nextFocus) { nextFocus.focus({preventScroll:true}); if (selection && typeof nextFocus.setSelectionRange === 'function') nextFocus.setSelectionRange(...selection); }
    else if (!background) document.getElementById('main-content')?.focus({preventScroll:true});
    return true;
    } catch (error) { if (error.name === 'AbortError') return false; throw error; }
    finally { if (userNavigationPending === sequence) userNavigationPending = 0; if (navigationController === controller) navigationController = null; }
  }
  async function refresh() { const p = current(); return navigate(p.view, { ...p, as_of:asOf() }, false, true); }
  async function call(path, body) {
    const response = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) });
    let result; try { result = await response.json(); } catch (_) { throw new Error('Máy chủ trả về phản hồi không hợp lệ.'); }
    if (!response.ok || result.ok === false) throw new Error(result.message || result.error || 'Không thể hoàn tất yêu cầu.');
    return result;
  }
  function usdToCents(value) {
    const raw = String(value ?? '').trim();
    if (!/^\d+(?:\.\d{1,2})?$/.test(raw)) throw new Error('Nhập số USD không âm, tối đa hai chữ số thập phân.');
    const [whole, fraction = ''] = raw.split('.');
    // Exact decimal arithmetic is equivalent to rounding up to cents for step=0.01,
    // without binary floating-point causing an extra cent for values such as 1.10.
    const cents = Math.ceil(Number(whole) * 100 + Number(fraction.padEnd(2, '0')));
    if (!Number.isSafeInteger(cents) || cents < 0) throw new Error('Số tiền vượt giới hạn hỗ trợ.');
    return cents;
  }
  const asOf = () => (window.matchMedia('(max-width: 900px)').matches ? document.getElementById('as-of-mobile')?.value : document.getElementById('as-of')?.value) || null;
  for (const input of [document.getElementById('as-of'), document.getElementById('as-of-mobile')]) input?.addEventListener('change', () => {
    for (const other of [document.getElementById('as-of'), document.getElementById('as-of-mobile')]) if (other && other !== input) other.value = input.value;
    const p = current(); navigate(p.view, { ...p, as_of:input.value, snapshot:'' }).catch(error => { for (const date of [document.getElementById('as-of'), document.getElementById('as-of-mobile')]) if (date) date.value = renderedAsOf; notify(error.message, true); });
  });
  const el = id => document.getElementById(id);
  function reportResult(id, text, error = false) { const node = el(id); if (node) { node.textContent = text; node.classList.toggle('error-text', error); } }
  async function openReport(id) {
    const result = await call('/api/reports/' + encodeURIComponent(id) + '/commitment', {});
    reportId = id; reportDigest = result.commitment?.digest || ''; preparedTransaction = null; walletAccount = ''; confirmedReceipt = result.anchoring_status === 'receipt_confirmed';
    el('commitment-digest').textContent = reportDigest || 'Chưa có digest';
    el('commitment-status').textContent = result.anchoring_status === 'receipt_confirmed' ? 'Receipt Fuji đã được xác nhận' : 'Chỉ lưu cục bộ, chưa xác nhận receipt';
    el('report-export-link').href = '/api/reports/' + encodeURIComponent(id) + '/export';
    el('report-json').value = ''; el('report-file').value = '';
    el('chain-prepared').hidden = true; el('chain-connected').hidden = true;
    el('transaction-hash').value = sessionStorage.getItem('lantern-tx:' + id) || '';
    reportResult('verify-result', ''); reportResult('chain-result', '');
    el('report-dialog').showModal();
  }
  async function verifyBundle() {
    const file = el('report-file').files[0], pasted = el('report-json').value.trim();
    if (file && pasted) throw new Error('Chọn file hoặc dán JSON, không dùng cả hai.');
    if (file && file.size > 5 * 1024 * 1024) throw new Error('File vượt giới hạn 5 MB.');
    const raw = file ? await file.text() : pasted;
    if (!raw || raw.length > 5 * 1024 * 1024) throw new Error('Chọn hoặc dán bản JSON không vượt 5 MB.');
    let bundle; try { bundle = JSON.parse(raw); } catch (_) { throw new Error('JSON không hợp lệ.'); }
    if (!bundle || typeof bundle.report !== 'object' || !bundle.report || typeof bundle.commitment !== 'object' || !bundle.commitment) throw new Error('Bản JSON cần có report và commitment.');
    const result = await call('/api/reports/' + encodeURIComponent(reportId) + '/verify', { report:bundle.report, commitment:bundle.commitment });
    const sameDigest = !!reportDigest && typeof bundle.commitment.digest === 'string' && reportDigest.toLowerCase() === bundle.commitment.digest.toLowerCase();
    if (!result.valid) reportResult('verify-result', 'Nội dung không khớp cam kết trong bản JSON: đã thay đổi hoặc sai phiên bản.', true);
    else if (!sameDigest) reportResult('verify-result', 'Bản JSON tự khớp cam kết của nó nhưng digest khác bản đang mở. Có thể là phiên bản khác; chưa xác nhận trùng bản hiện tại.', true);
    else reportResult('verify-result', 'Nội dung khớp cam kết và digest của bản đang mở. Điều này không chứng minh nội dung là đúng.');
  }
  async function prepareChain() {
    preparedTransaction = null; walletAccount = ''; el('chain-prepared').hidden = true; el('chain-connected').hidden = true;
    if (confirmedReceipt) throw new Error('Receipt của phiên bản này đã được xác nhận.');
    if (sessionStorage.getItem('lantern-tx:' + reportId)) throw new Error('Phiên này đã gửi giao dịch. Hãy kiểm tra receipt bằng hash đã lưu trước khi làm gì thêm.');
    const result = await call('/api/reports/' + encodeURIComponent(reportId) + '/transaction', {});
    const tx = result.transaction;
    if (result.chain_id !== '0xa869' || !tx || !/^0x[0-9a-fA-F]{40}$/.test(tx.to) || !/^0x[0-9a-fA-F]+$/.test(tx.data) || tx.value !== '0x0') throw new Error('Giao dịch Fuji không hợp lệ; chưa kết nối ví.');
    preparedTransaction = { to:tx.to, data:tx.data, value:tx.value };
    el('chain-contract').textContent = tx.to; el('chain-prepared').hidden = false;
    reportResult('chain-result', result.message || 'Đã chuẩn bị. Chưa gửi giao dịch.');
  }
  async function connectWallet() {
    if (!preparedTransaction) throw new Error('Hãy chuẩn bị giao dịch trước.');
    if (!window.ethereum?.request) throw new Error('Không tìm thấy ví trình duyệt. Cài ví tương thích và tự kết nối.');
    const accounts = await window.ethereum.request({ method:'eth_requestAccounts' });
    if (!Array.isArray(accounts) || !/^0x[0-9a-fA-F]{40}$/.test(accounts[0] || '')) throw new Error('Chưa có tài khoản ví được chọn.');
    await window.ethereum.request({ method:'wallet_switchEthereumChain', params:[{ chainId:'0xa869' }] });
    const chain = await window.ethereum.request({ method:'eth_chainId' });
    if (String(chain).toLowerCase() !== '0xa869') throw new Error('Ví chưa ở mạng Fuji.');
    walletAccount = accounts[0]; el('chain-account').textContent = walletAccount; el('chain-connected').hidden = false;
    reportResult('chain-result', 'Đã kết nối Fuji. Chưa gửi; hãy kiểm tra contract và xác nhận giao dịch trong ví.');
  }
  async function checkReceipt() {
    const hash = el('transaction-hash').value.trim();
    if (!/^0x[0-9a-fA-F]{64}$/.test(hash)) throw new Error('Transaction hash phải gồm 0x và 64 ký tự hex.');
    sessionStorage.setItem('lantern-tx:' + reportId, hash);
    try {
      const result = await call('/api/reports/' + encodeURIComponent(reportId) + '/anchor', { transaction_hash:hash });
      confirmedReceipt = true;
      el('commitment-status').textContent = 'Receipt Fuji đã được xác nhận';
      reportResult('chain-result', result.message || 'Receipt khớp contract và digest.');
    } catch (error) { reportResult('chain-result', (error.message || 'Chưa xác minh được receipt.') + ' Hash được giữ trong phiên để bạn kiểm tra lại.', true); }
  }
  async function sendChain() {
    if (!preparedTransaction || !walletAccount || !window.ethereum?.request) throw new Error('Hãy chuẩn bị và kết nối ví trước.');
    if (confirmedReceipt || sessionStorage.getItem('lantern-tx:' + reportId)) throw new Error('Phiên này đã có giao dịch; hãy kiểm tra receipt thay vì gửi lại.');
    const chain = await window.ethereum.request({ method:'eth_chainId' });
    const accounts = await window.ethereum.request({ method:'eth_accounts' });
    if (String(chain).toLowerCase() !== '0xa869' || !Array.isArray(accounts) || !accounts.some(a => a.toLowerCase() === walletAccount.toLowerCase())) throw new Error('Ví hoặc mạng đã thay đổi. Hãy kết nối lại Fuji.');
    const hash = await window.ethereum.request({ method:'eth_sendTransaction', params:[{ ...preparedTransaction, from:walletAccount }] });
    if (!/^0x[0-9a-fA-F]{64}$/.test(hash || '')) throw new Error('Ví không trả transaction hash hợp lệ.');
    sessionStorage.setItem('lantern-tx:' + reportId, hash); el('transaction-hash').value = hash;
    reportResult('chain-result', 'Đã gửi giao dịch. Đang kiểm tra receipt; chưa khẳng định đã neo.');
    await checkReceipt();
  }
  async function search(query) {
    if (!query.trim()) return;
    await navigate('companies', { q: query.trim() });
  }
  function stopAudio() {
    if (audio) { audio.pause(); audio.currentTime = 0; audio = null; }
    if (audioUrl) { URL.revokeObjectURL(audioUrl); audioUrl = null; }
    if ('speechSynthesis' in window) window.speechSynthesis.cancel();
  }
  async function speak(text) {
    stopAudio();
    const response = await fetch('/api/speech', { method:'POST', headers:{ 'Content-Type':'application/json' }, body:JSON.stringify({ text }) });
    if (!response.ok) {
      let message = 'Không phát được giọng từ dịch vụ. Có thể chọn giọng thiết bị.';
      try { const data = await response.json(); message = data.message || data.error || message; } catch (_) {}
      throw new Error(message);
    }
    const blob = await response.blob(); audioUrl = URL.createObjectURL(blob); audio = new Audio(audioUrl);
    audio.onended = stopAudio;
    await audio.play();
  }
  const voiceRecipient = () => document.getElementById('voice-consent')?.dataset.recipient || '';
  const voiceConsentKey = () => 'mira-voice-consent:' + voiceRecipient();
  function updateRecordingUi() {
    const state = document.getElementById('recording-state');
    const microphone = document.querySelector('[data-action="record"]');
    if (microphone) microphone.disabled = !!recordingSession;
    if (state) {
      state.hidden = !recordingSession;
      const label = state.querySelector('span:nth-child(2)');
      if (label && recordingSession) label.textContent = recordingSession.phase === 'recording' ? 'Đang ghi âm' : 'Đang xử lý âm thanh';
      const stop = state.querySelector('[data-action="stop-recording"]');
      if (stop) stop.hidden = !recordingSession || recordingSession.phase !== 'recording';
    }
  }
  function releaseSession(session) {
    clearTimeout(session.timer);
    session.stream?.getTracks().forEach(track => track.stop()); session.stream = null;
  }
  function finishSession(session) {
    releaseSession(session);
    if (recordingSession === session) recordingSession = null;
    updateRecordingUi();
  }
  async function asMonoWav(webm) {
    const Decoder = window.AudioContext || window.webkitAudioContext;
    const Offline = window.OfflineAudioContext || window.webkitOfflineAudioContext;
    if (!Decoder || !Offline) throw new Error('Trình duyệt không hỗ trợ xử lý âm thanh an toàn. Hãy nhập văn bản.');
    const context = new Decoder();
    let decoded;
    try { decoded = await context.decodeAudioData(await webm.arrayBuffer()); }
    catch (_) { throw new Error('Không giải mã được âm thanh. Hãy thử lại hoặc nhập văn bản.'); }
    finally { await context.close().catch(() => {}); }
    if (!Number.isFinite(decoded.duration) || decoded.duration <= 0 || decoded.duration > 30) throw new Error('Âm thanh phải dài hơn 0 và không quá 30 giây. Hãy ghi lại ngắn hơn.');
    const sampleRate = 16000, frames = Math.ceil(decoded.duration * sampleRate);
    const offline = new Offline(1, frames, sampleRate);
    const source = offline.createBufferSource(); source.buffer = decoded;
    source.connect(offline.destination); source.start();
    const mono = (await offline.startRendering()).getChannelData(0);
    const wav = new ArrayBuffer(44 + mono.length * 2), view = new DataView(wav);
    const ascii = (offset, text) => { for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i)); };
    ascii(0, 'RIFF'); view.setUint32(4, 36 + mono.length * 2, true); ascii(8, 'WAVE');
    ascii(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
    view.setUint32(24, sampleRate, true); view.setUint32(28, sampleRate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
    ascii(36, 'data'); view.setUint32(40, mono.length * 2, true);
    for (let i = 0; i < mono.length; i++) { const sample = Math.max(-1, Math.min(1, Number.isFinite(mono[i]) ? mono[i] : 0)); view.setInt16(44 + i * 2, sample < 0 ? sample * 32768 : sample * 32767, true); }
    return new Blob([wav], { type:'audio/wav' });
  }
  async function startRecording() {
    const recipient = voiceRecipient();
    if (!recipient || !recipient.startsWith('https://')) throw new Error('Chưa cấu hình nhà cung cấp âm thanh; micro chưa được bật.');
    if (!navigator.mediaDevices?.getUserMedia || !window.MediaRecorder) throw new Error('Trình duyệt không hỗ trợ ghi âm. Hãy nhập văn bản.');
    if (!MediaRecorder.isTypeSupported('audio/webm')) throw new Error('Trình duyệt không hỗ trợ định dạng audio/webm. Hãy nhập văn bản.');
    if (recordingSession) throw new Error('Vui lòng chờ bản ghi hiện tại xử lý xong.');
    const session = { phase:'requesting', stream:null, recorder:null, chunks:[], bytes:0, discard:false, tooLarge:false, timer:null, controller:new AbortController() };
    recordingSession = session; updateRecordingUi();
    try {
    session.stream = await navigator.mediaDevices.getUserMedia({ audio:true });
    if (session.discard) { finishSession(session); return; }
    session.recorder = new MediaRecorder(session.stream, { mimeType:'audio/webm' });
    session.recorder.ondataavailable = event => {
      if (!event.data.size) return;
      session.bytes += event.data.size;
      if (session.bytes > 5 * 1024 * 1024) { session.tooLarge = true; if (session.recorder.state !== 'inactive') session.recorder.stop(); return; }
      session.chunks.push(event.data);
    };
    session.recorder.onerror = () => { session.discard = true; session.controller.abort(); finishSession(session); notify('Ghi âm không thành công. Hãy thử lại hoặc nhập văn bản.', true); };
    session.recorder.onstop = async () => {
      releaseSession(session); session.phase = 'processing'; updateRecordingUi();
      try {
        if (session.discard) return;
        if (session.tooLarge) throw new Error('Bản ghi vượt 5 MB. Hãy ghi lại ngắn hơn.');
        if (!session.bytes) throw new Error('Không có âm thanh để phiên âm.');
        notify('Đang chuyển giọng nói thành văn bản…');
        const wav = await asMonoWav(new Blob(session.chunks, {type:'audio/webm'}));
        if (session.discard) return;
        const response = await fetch('/api/transcribe', { method:'POST', headers:{ 'Content-Type':'audio/wav', Accept:'application/json' }, body:wav, signal:session.controller.signal });
        let result; try { result = await response.json(); } catch (_) { throw new Error('Phản hồi phiên âm không hợp lệ.'); }
        if (!response.ok || result.ok === false) throw new Error(result.message || result.error || 'Không phiên âm được.');
        const text = result.text || result.transcript || '';
        if (!text) throw new Error('Không nhận được bản chép. Hãy thử lại hoặc nhập văn bản.');
        const input = document.getElementById('chat-input');
        if (input) { input.value = input.value ? input.value + '\n' + text : text; input.focus(); }
        notify('Đã chép lời. Hãy kiểm tra và chỉnh sửa trước khi gửi.');
      } catch (error) { if (!session.discard && error.name !== 'AbortError') notify(error.message || 'Không phiên âm được.', true); }
      finally { finishSession(session); }
    };
    session.recorder.start(250); session.phase = 'recording'; updateRecordingUi();
    session.timer = setTimeout(() => { if (session.recorder?.state === 'recording') session.recorder.stop(); }, 29500);
    } catch (error) { finishSession(session); throw error; }
  }
  function refreshInstall() {
    const button = document.getElementById('install-button'), hint = document.getElementById('install-hint');
    if (!button) return;
    button.disabled = !installEvent;
    button.title = installEvent ? 'Cài LANTERN vào thiết bị' : 'Trình duyệt chưa cung cấp tùy chọn cài đặt';
    if (hint) hint.textContent = installEvent ? 'Trình duyệt đã sẵn sàng cài ứng dụng.' : 'Tùy chọn cài đặt chưa khả dụng trên trình duyệt này.';
  }
  window.addEventListener('beforeinstallprompt', event => { event.preventDefault(); installEvent = event; refreshInstall(); });
  window.addEventListener('appinstalled', () => { installEvent = null; refreshInstall(); notify('Ứng dụng đã được cài đặt.'); });
  window.addEventListener('popstate', () => { const p = current(); const dateValue = p.as_of || initialAsOf; for (const date of [el('as-of'), el('as-of-mobile')]) if (date) date.value = dateValue; navigate(p.view, { ...p, as_of:dateValue }, false).catch(error => notify(error.message, true)); });
  document.addEventListener('submit', async event => {
    const form = event.target.closest('form[data-form]'); if (!form) return;
    event.preventDefault();
    if (busy) return;
    const button = form.querySelector('[type="submit"]'); if (button) button.disabled = true; busy = true;
    try {
      const data = new FormData(form);
      switch (form.dataset.form) {
        case 'search': await search(String(data.get('query') || '')); break;
        case 'chat': {
          const message = String(data.get('message') || '').trim(); if (!message) break;
          const result = await call('/api/chat', { message, as_of:asOf() });
          const input = document.getElementById('chat-input');
          if (input && input.value.trim() === message) input.value = '';
          await refresh();
          if (result.message) notify(result.message);
          document.getElementById('mira-dock')?.classList.add('open');
          document.getElementById('mira-messages')?.scrollTo(0, 999999);
          break;
        }
        case 'investigation': {
          const draftKey = current().selected;
          const submittedQuery = String(data.get('query') || '').trim();
          const result = await call('/api/investigations', { company_id:String(data.get('company_id')), query:String(data.get('query') || '').trim(), as_of:asOf() });
          const input = document.getElementById('investigate-query');
          if (current().selected === draftKey && input && input.value.trim() === submittedQuery) { input.value = ''; investigationDrafts.delete(draftKey); }
          else if ((investigationDrafts.get(draftKey) || '').trim() === submittedQuery) investigationDrafts.delete(draftKey);
          notify(result.message || 'Đã gửi yêu cầu nghiên cứu.'); await refresh(); break;
        }
        case 'compare': {
          const ids = data.getAll('ids'); if (ids.length < 2 || ids.length > 4) throw new Error('Chọn từ hai đến bốn doanh nghiệp.');
          await navigate('compare', { ids:ids.join(',') }); break;
        }
        case 'reconcile-budget': {
          const reservationId = String(data.get('reservation_id') || '');
          if (!reservationId || !data.has('confirmed')) throw new Error('Hãy xác nhận đã kiểm tra hóa đơn trước khi đối soát.');
          const actual_cents = usdToCents(data.get('actual_usd'));
          const result = await call('/api/budget/' + encodeURIComponent(reservationId) + '/settle', { actual_cents, confirmed:true });
          notify(result.message || 'Đã lưu đối soát ngân sách.'); await refresh(); break;
        }
        case 'budget-limit': {
          if (!data.has('confirmed')) throw new Error('Hãy xác nhận trước khi thay đổi giới hạn.');
          const cents = usdToCents(data.get('amount_usd'));
          const result = await call('/api/budget/limit', { cents, confirmed:true });
          notify(result.message || 'Đã lưu giới hạn ngân sách.'); await refresh(); break;
        }
        case 'evidence-add': case 'risk-assess': case 'risk-identity': case 'risk-retract': case 'evolution-feedback': {
          let body=Object.fromEntries(data), path;
          if(form.dataset.form==='evidence-add') { path='/api/evidence'; body.published_at=body.published_at || null; }
          else if(form.dataset.form==='risk-assess') { path='/api/risk/assessments'; body.severity=Number(body.severity); body.dimension=form.elements.namedItem('evidence_id').selectedOptions[0]?.dataset.dimension; delete body.confirmed; }
          else if(form.dataset.form==='risk-identity') { path='/api/risk/identity'; body.confirmed=body.confirmed==='true'; }
          else if(form.dataset.form==='risk-retract') { path='/api/risk/assessments/'+encodeURIComponent(body.entry_id)+'/retract'; body={analyst:body.analyst,reason:body.rationale}; }
          else { path='/api/evolution/runs/'+encodeURIComponent(body.run_id)+'/feedback'; body={feedback:body.feedback}; }
          const result=await call(path,body); form.reset(); formDrafts.delete(formKey(current(),form)); notify(result.message || 'Đã lưu.'); await refresh(); break;
        }
      }
    } catch (error) { notify(error.message || 'Không thể hoàn tất yêu cầu.', true); if (form.dataset.form === 'chat') { try { await refresh(); } catch (_) {} } }
    finally { busy = false; if (button?.isConnected) button.disabled = false; }
  });
  document.addEventListener('click', async event => {
    const target = event.target.closest('[data-action],a[data-view]'); if (!target) return;
    if (target.tagName === 'A' && target.dataset.view) {
      if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      try { await navigate(target.dataset.view, { selected:target.dataset.selected }); } catch (error) { notify(error.message, true); }
      return;
    }
    const action = target.dataset.action;
    const exclusive=['watch','review-evidence','budget-bound','cancel-job','search-web','scout-run','scout-toggle','scout-resume','source-toggle','source-run','source-resume','risk-snapshot','evolution-evaluate','evolution-promote','evolution-rollback'].includes(action);
    if(exclusive && busy) return;
    if(exclusive) { busy=true; target.disabled=true; }
    try {
      switch (action) {
        case 'page': { const p=current(); await navigate(target.dataset.targetView || p.view,{...p,page:target.dataset.page}); break; }
        case 'risk-open': await navigate('risk',{snapshot:target.dataset.id}); break;
        case 'risk-snapshot': { const company_ids=JSON.parse(el('risk-cohort')?.value || '[]'); const result=await call('/api/risk/snapshots',{company_ids,as_of:asOf()}); notify(result.message); await navigate('risk',{snapshot:result.snapshot.id}); break; }
        case 'evolution-evaluate': case 'evolution-promote': case 'evolution-rollback': { const suffix=action.replace('evolution-',''); const result=await call('/api/evolution/'+suffix,{version:Number(target.dataset.version)}); notify(result.message); await refresh(); break; }
        case 'source-toggle': case 'source-run': case 'source-resume': { const suffix=action==='source-toggle'?'enabled':action==='source-run'?'run':'resume-after-review'; const result=await call('/api/scout/sources/'+encodeURIComponent(target.dataset.id)+'/'+suffix,{enabled:target.dataset.enabled!=='true'}); notify(result.message || 'Đã cập nhật lịch nguồn.'); await refresh(); break; }
        case 'menu': { const el = document.getElementById('navigation'); el?.classList.toggle('open'); target.setAttribute('aria-expanded', String(el?.classList.contains('open'))); break; }
        case 'mira': document.getElementById('mira-dock')?.classList.add('open'); target.setAttribute('aria-expanded','true'); document.getElementById('chat-input')?.focus(); break;
        case 'mira-close': document.getElementById('mira-dock')?.classList.remove('open'); document.querySelector('.mira-mobile')?.setAttribute('aria-expanded','false'); document.querySelector('.mira-mobile')?.focus(); break;
        case 'focus-search': document.getElementById('command-input')?.focus(); break;
        case 'suggest': { const input = document.getElementById('chat-input'); if (input) { if (!input.value.trim()) input.value = target.dataset.text || ''; else notify('Bản nháp vẫn được giữ.'); input.focus(); } break; }
        case 'scout-run': { target.disabled = true; try { const result = await call('/api/scout/run', {}); notify(result.message || 'Đã gửi yêu cầu Scout.'); await refresh(); } finally { if (target.isConnected) target.disabled = false; } break; }
        case 'scout-toggle': { target.disabled = true; try { const result = await call('/api/scout/enabled', { enabled:target.dataset.enabled !== 'true' }); notify(result.message || 'Đã cập nhật Scout.'); await refresh(); } finally { if (target.isConnected) target.disabled = false; } break; }
        case 'scout-resume': { if (!confirm('Bạn đã rà soát giới hạn và điều khoản nguồn trước khi tiếp tục?')) break; target.disabled = true; try { const result = await call('/api/scout/resume-after-review', {}); notify(result.message || 'Đã ghi nhận rà soát.'); await refresh(); } finally { if (target.isConnected) target.disabled = false; } break; }
        case 'search-web': { const query = (target.dataset.query || '').trim(); if (!query) break; target.disabled = true; try { const result = await call('/api/search', { query, as_of:asOf() }); notify(result.message || 'Đã gửi yêu cầu tìm bằng AI.'); await refresh(); } finally { if (target.isConnected) target.disabled = false; } break; }
        case 'watch': { target.disabled = true; const result = await call('/api/watchlist', { company_id:target.dataset.id, watched:target.dataset.watched !== 'true' }); notify(result.message || 'Đã cập nhật danh sách theo dõi.'); await refresh(); break; }
        case 'review-evidence': { target.disabled = true; const decision = target.dataset.decision; if (!['subject_confirmed','not_about_company'].includes(decision)) throw new Error('Lựa chọn đối chiếu không hợp lệ.'); const result = await call('/api/evidence/' + encodeURIComponent(target.dataset.id) + '/review', { decision }); notify(result.message || 'Đã lưu lựa chọn đối chiếu chủ thể; nội dung chưa được kiểm chứng.'); await refresh(); break; }
        case 'budget-bound': { target.disabled = true; const result = await call('/api/budget/' + encodeURIComponent(target.dataset.id) + '/bound', {}); notify(result.message || 'Đã lưu dự phòng ước tính; đây không phải hóa đơn.'); await refresh(); break; }
        case 'cancel-job': { target.disabled = true; const result = await call('/api/jobs/' + encodeURIComponent(target.dataset.id) + '/cancel', {}); notify(result.message || 'Đã gửi yêu cầu hủy tác vụ.'); await refresh(); break; }
        case 'report-commitment': { target.disabled = true; try { await openReport(target.dataset.id); } finally { if (target.isConnected) target.disabled = false; } break; }
        case 'report-close': el('report-dialog').close(); break;
        case 'report-verify': { target.disabled = true; try { await verifyBundle(); } catch (error) { reportResult('verify-result', error.message || 'Không kiểm tra được.', true); } finally { target.disabled = false; } break; }
        case 'chain-prepare': { target.disabled = true; try { await prepareChain(); } catch (error) { reportResult('chain-result', error.message || 'Không chuẩn bị được.', true); } finally { target.disabled = false; } break; }
        case 'chain-connect': { target.disabled = true; try { await connectWallet(); } catch (error) { reportResult('chain-result', error.message || 'Không kết nối được ví.', true); } finally { target.disabled = false; } break; }
        case 'chain-send': { target.disabled = true; try { await sendChain(); } catch (error) { reportResult('chain-result', 'Chưa xác nhận được kết quả gửi. Kiểm tra lịch sử ví trước khi thử lại. ' + (error.message || ''), true); } finally { target.disabled = false; } break; }
        case 'chain-retry': { target.disabled = true; try { await checkReceipt(); } catch (error) { reportResult('chain-result', error.message || 'Không kiểm tra được receipt.', true); } finally { target.disabled = false; } break; }
        case 'record': if (!voiceRecipient()) throw new Error('Chưa cấu hình nhà cung cấp âm thanh; micro chưa được bật.'); document.getElementById('mira-dock')?.classList.add('open'); document.querySelector('.mira-mobile')?.setAttribute('aria-expanded', 'true'); if (sessionStorage.getItem(voiceConsentKey()) === 'yes') await startRecording(); else document.getElementById('voice-consent').showModal(); break;
        case 'voice-decline': document.getElementById('voice-consent').close(); break;
        case 'voice-accept': if (!voiceRecipient()) throw new Error('Chưa cấu hình nhà cung cấp âm thanh; micro chưa được bật.'); document.getElementById('voice-consent').close(); sessionStorage.setItem(voiceConsentKey(),'yes'); await startRecording(); break;
        case 'forget-voice': sessionStorage.removeItem(voiceConsentKey()); notify('Đã đặt lại lựa chọn ghi âm.'); break;
        case 'stop-recording': if (recordingSession?.recorder?.state === 'recording') recordingSession.recorder.stop(); break;
        case 'stop-audio': stopAudio(); notify('Đã dừng âm thanh. Tác vụ nghiên cứu vẫn tiếp tục.'); break;
        case 'speak': await speak(target.dataset.text || ''); break;
        case 'device-speak': { if (!('speechSynthesis' in window)) throw new Error('Thiết bị không hỗ trợ đọc văn bản.'); stopAudio(); const utterance = new SpeechSynthesisUtterance(target.dataset.text || ''); utterance.lang = 'vi-VN'; window.speechSynthesis.speak(utterance); break; }
        case 'install': if (installEvent) { await installEvent.prompt(); await installEvent.userChoice; installEvent = null; refreshInstall(); } break;
      }
    } catch (error) { notify(error.message || 'Không thể hoàn tất yêu cầu.', true); }
    finally { if(exclusive) busy=false; if(target.isConnected && !['install'].includes(action)) target.disabled=false; }
  });
  document.addEventListener('keydown', event => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); document.getElementById('command-input')?.focus(); }
    if (event.key === 'Escape') {
      stopAudio();
      const session = recordingSession;
      if (session) { session.discard=true; session.controller.abort(); if (session.recorder?.state === 'recording') session.recorder.stop(); releaseSession(session); }
      if (document.getElementById('voice-consent')?.open) document.getElementById('voice-consent').close();
      if (document.getElementById('report-dialog')?.open) document.getElementById('report-dialog').close();
      document.getElementById('navigation')?.classList.remove('open');
      document.getElementById('mira-dock')?.classList.remove('open');
      document.querySelector('.mobile-menu')?.setAttribute('aria-expanded', 'false');
      document.querySelector('.mira-mobile')?.setAttribute('aria-expanded', 'false');
    }
  });
  window.addEventListener('pagehide', () => { const session = recordingSession; if (session) { session.discard = true; session.controller.abort(); if (session.recorder?.state === 'recording') session.recorder.stop(); releaseSession(session); } stopAudio(); });
  // Poll only local snapshots. Preserve drafts and avoid replacing a focused form.
  setInterval(async () => {
    const focusedForm = document.activeElement?.closest('form');
    if (polling || busy || userNavigationPending || document.hidden || el('report-dialog')?.open || el('voice-consent')?.open || (focusedForm && workspace.contains(focusedForm)) || recordingSession) return;
    polling = true;
    const startedAt = navigationSequence;
    try {
      const params = new URLSearchParams(); if (asOf()) params.set('as_of', asOf());
      const response = await fetch('/api/revision', { cache:'no-store' }); if (!response.ok) return;
      const state = await response.json();
      const activeForm = document.activeElement?.closest('form');
      if (startedAt !== navigationSequence || userNavigationPending || busy || recordingSession || el('report-dialog')?.open || el('voice-consent')?.open || (activeForm && workspace.contains(activeForm))) return;
      const signature = state.revision;
      if ((pollSignature === null || signature !== pollSignature) && !(await refresh())) return;
      pollSignature = signature;
    } catch (_) { /* A later poll will retry; mutations are never retried automatically. */ }
    finally { polling = false; }
  }, 5000);
  if ('serviceWorker' in navigator && window.isSecureContext) navigator.serviceWorker.register('/sw.js').catch(() => {});
})();
