import { loadTemplate, initTabs } from "./common.js";
import { localizeChangeStatus, localizeSpan } from "./localize.js";

const tabs = ["tokens-tab", "limits-tab"];
let tokensTemplate = null;
let limitsTemplate = null;
let limitsDict = null;
let tokensDict = null;
let origLimits = null;
const refreshInterval = 2000000;

async function getTokens() {
    return await fetch("/profile/tokens/list").then(r => r.json());
};

async function getLimits(){
    return await fetch("/profile/limits/get").then(r => r.json());
}

async function getMyChanges(){
    return await fetch("/profile/limits/changes").then(r => r.json())
}

async function postEnable(token) {
    const body = {token: token}
    return await fetch("/profile/tokens/enable",
    {
        method: "POST",
        body: JSON.stringify(body)
    });
}

async function postDisable(token) {
    const body = {token: token}
    return await fetch("/profile/tokens/disable",
    {
        method: "POST",
        body: JSON.stringify(body)
    });
}

async function postLimitsChange(changes){
    return await fetch("/profile/limits",
    {
        method: "POST",
        body: JSON.stringify(changes)
    });
}

async function postChangeCancel(currency){
    return await fetch("/profile/limits/cancel",
    {
        method: "POST",
        body: JSON.stringify(currency)
    });
}

function changeCancelHandler(currency){
    return async function(){
        await postChangeCancel(currency);
        loadLimits();
    }
}

function getCurName(val){
    if (val.currency == null) {
        return "null"
    } else if (typeof val.currency === 'object') {
        return val.currency[Object.keys(val.currency)[0]].ticker
    } else {
        return val.currency
    }
}

async function initTemplates() {
    const [tokensTemp, tokensD, limitsTemp, limitsD] = await Promise.allSettled([
        await loadTemplate("/templates/token.html.hbs"),
        await fetch("/lang/token.json").then(r => r.json()),
        await loadTemplate("/templates/limits.html.hbs"),
        await fetch("/lang/limits.json").then(r => r.json()),
    ]);

    const status = {InProgress: {confirmations: 0, rejections: 0}}
    tokensTemplate = tokensTemp.value;
    limitsTemplate = limitsTemp.value;
    limitsDict = limitsD.value;
    tokensDict = tokensD.value;
    Handlebars.registerHelper('tokenFormatName', function () { return this.token.ticker; });
    Handlebars.registerHelper('limitsFormatName', function () { return getCurName(this) });
    Handlebars.registerHelper('changesFormatName', function () { return getCurName(this) });
    Handlebars.registerHelper('changeStatus', function () { return localizeChangeStatus(this.status) });
    Handlebars.registerHelper('localizeSpan', function () { return localizeSpan(this.limit.span)})
}

function setLimit(limit){
    const cur = getCurName(limit);
    const span = limit.limit_info.limit.span;
    const value = limit.limit_info.limit.amount;
    const valEl = document.getElementById(cur + "-value");
    const spanEl = document.getElementById(cur + "-span");
    valEl.value = value; 
    spanEl.value = span;
}

async function checkboxHandler(event, token) {
    if (event.currentTarget.checked) {
        const resp = await postEnable(token);
        loadTokens();
    } else {
        const resp = await postDisable(token);
        loadTokens();
    }
}

async function loadTokens() {
    const tokens = await getTokens();
    const tokensDrawUpdate = tokensTemplate({tokens: tokens.tokens, lang: tokensDict});
    const tokensElem = document.getElementById("tokens-tab-body");
    tokensElem.innerHTML = tokensDrawUpdate;
    const tokensArray = tokens.tokens;
    tokensArray.forEach(token => {
        const checkbox = document.getElementById('checkbox-'+token.token.ticker)
        checkbox.addEventListener('change', (event) => checkboxHandler(event, token.token))
    });
}

async function loadLimits(){
    const limits = await getLimits();
    const changes = await getMyChanges();
    origLimits = limits;
    const limitsDrawUpdate = limitsTemplate({limits: limits, changes: changes, lang: limitsDict});
    const limitsElem = document.getElementById("limits-tab-body");
    limitsElem.innerHTML = limitsDrawUpdate;
    limits.forEach(limit => { 
        setLimit(limit) 
        setChangeHandlers(getCurName(limit))
    });

    changes.forEach(change => {
        const btn = document.getElementById(change.id + "-btn");
        btn.onclick = changeCancelHandler(change.currency)
    })
}

function setChangeHandlers(cur) {
    const valEl = document.getElementById(cur + "-value");
    const spanEl = document.getElementById(cur + "-span");
    valEl.oninput = checkLimitsChange;
    spanEl.addEventListener("change", (event) => checkLimitsChange());
}

function commitChanges(changes){
    return async function(){
        await postLimitsChange(changes)
        loadLimits()
    }
}

function checkLimitsChange() {
    var changes = [];
    origLimits.forEach(ol => {
        const cur = getCurName(ol);
        const valEl = document.getElementById(cur + "-value");
        const spanEl = document.getElementById(cur + "-span");
        if (!(valEl.value == ol.limit_info.limit.amount && spanEl.value == ol.limit_info.limit.span)){
            changes.push({
                currency: ol.currency,
                limit: {
                    amount: parseInt(valEl.value),
                    span: spanEl.value
                }
            })
        }
    })
    if (changes.length !== 0) {
        const commitBtn = document.getElementById("commit-limits-btn");
        commitBtn.style.display = 'inline-flex'
        commitBtn.onclick = commitChanges(changes);
    }
}

async function updateLoop() {
    await Promise.allSettled([
        loadTokens(),
        loadLimits(),
    ]);
    // await new Promise((resolve) => setTimeout(resolve, refreshInterval));
    // updateLoop();
}

async function init() {
    initTabs(tabs);
    await initTemplates();
    updateLoop();
};


document.addEventListener("headerLoaded", init);
