import { loadTemplate, formattedCurrencyValue, formattedElapsedTime, currencyNameToCurrency } from "./common.js";

let balanceTemplate = null;
let historyTemplate = null;
let dict = null;
const refreshInterval = 20000;
const historyPageSize = 50;
let historyPagesToLoad = 1;

async function getBalances() {
    return await fetch("/balance").then(r => r.json());
};

async function getHistory(skip, take) {
    return fetch(`/history/${skip}/${take}`).then(r => r.json());
}

async function getCourseForCurrency(currency) {
    return await fetch("/ticker",
        {
            method: "POST",
            body: JSON.stringify(currency)
        }).then(r => r.json());
};

async function initTemplates() {

    const [balanceTemp, historyTemp, dictTemp] = await Promise.allSettled([
        loadTemplate("/templates/balance.html.hbs"),
        loadTemplate("/templates/history.html.hbs"),
        fetch("/lang/overview-extra.json").then(r => r.json())
    ]);

    balanceTemplate = balanceTemp.value;
    historyTemplate = historyTemp.value;
    dict = dictTemp.value;

    Handlebars.registerHelper('isDeposit', (historyItem) => historyItem.type === "deposit");
    Handlebars.registerHelper('isWithdrawal', (historyItem) => historyItem.type === "withdrawal");
    Handlebars.registerHelper('formatCurrencyValue', function () {
        if (typeof this.currency === 'object') {
            return formattedCurrencyValue(this.currency.ERC20.ticker, this.value);
        } else {
            return formattedCurrencyValue(this.currency, this.value);
        }
    });
    Handlebars.registerHelper('currencyFullName', (currencyName) => {
        switch (currencyName) {
            case "BTC":
                return "Bitcoin";
            case "ETH":
                return "Ethereum";
            default:
                return currencyName;
        }
    })
    Handlebars.registerHelper('formatCurrencyName', function () {
        if (typeof this.currency === 'object') {
            return this.currency.ERC20.ticker;
        } else {
            return this.currency;
        }
    });
    Handlebars.registerHelper('truncate', (text, n) => `${text.slice(0, n)} ...`);

    Handlebars.registerHelper('formattedElapsedTime', function () {
        return formattedElapsedTime(this.date);
    });
    Handlebars.registerHelper('isInProgress', (req_confirmations, confirmations) => req_confirmations > confirmations);
    Handlebars.registerHelper('toLowerCase', (s) => s.toLowerCase());
}

async function loadBalance() {
    const balances = await getBalances();
    const balanceDrawUpdate = balanceTemplate({ balances: balances.balances, lang: dict });
    const balancesElem = document.getElementById("balances");
    balancesElem.innerHTML = balanceDrawUpdate;
}

async function loadHistory() {
    function mapHistory(historyItem) {
        const isDeposit = historyItem.type == "deposit";
        const timeStamp = timeStampToTime(Math.round(Date.parse(historyItem.date) / 1000));
        const currencyName = typeof historyItem.currency === 'object' ? historyItem.currency.ERC20.ticker : historyItem.currency;
        if (isDeposit) {
            let explorerLink;
            switch (currencyName) {
                case "BTC":
                    explorerLink = `https://mempool.space/tx/${historyItem.txid.txid}`;
                    break;
                default:
                    explorerLink = `https://etherscan.io/tx/${historyItem.txid.txid}`;
                    break;
            }
            return {
                timeStamp: timeStamp,
                valueToShow: `+${formattedCurrencyValue(currencyName, historyItem.value)} ${currencyName}`,
                hash: historyItem.txid.txid,
                explorerLink: explorerLink,
                flowClass: "is-deposit",
                arrow: "mdi-arrow-collapse-down",
                flowType: `${dict.deposit} ${currencyName}`
            }
        } else {
            let explorerLink;
            switch (currencyName) {
                case "BTC":
                    explorerLink = "";
                    break;
                default:
                    explorerLink = `https://etherscan.io/tx/${historyItem.txid.txid}`;
                    break;
            }
            return {
                timeStamp: timeStamp,
                valueToShow: `-${formattedCurrencyValue(currencyName, historyItem.value)} ${currencyName}`,
                hash: historyItem.txid.txid,
                explorerLink: explorerLink,
                flowClass: "is-withdrawal",
                arrow: "mdi-arrow-up",
                flowType: `${dict.withdraw} ${currencyName}`
            }
        }
    }
    const historyBTCpred = await getHistory(0, 20);
    const mappedHistory = historyBTCpred.history_items.map(mapHistory);
    const historyDrawUpdate = historyTemplate({ histories: mappedHistory });
    const historyElem = document.getElementById("history-table");
    historyElem.innerHTML = historyDrawUpdate;
    enableCopyBtns(historyElem);
}

function enableCopyBtns(historyElem) {
    let tableRows = historyElem.getElementsByTagName('tr');
    for (const row of tableRows) {
        const txId = row.getElementsByTagName('a')[0].innerHTML;
        const copyBtn = row.getElementsByTagName('button')[0];
        copyBtn.addEventListener("click", () => navigator.clipboard.writeText(txId).then(() => { }, err =>
            console.error('Could not copy text: ', err)
        ));
        tippy(copyBtn, {
            content: dict.copied,
            trigger: "click",
            hideOnClick: false,
            onShow(instance) {
                setTimeout(() => instance.hide(), 1000);
            }
        });
    };
}

function enableDepositWithdrawBtns(balancesElem) {
    let balanceItems = balancesElem.getElementsByClassName('balances-item');
    for (const item of balanceItems) {
        const [depositBtn, withdrawBtn] = item.getElementsByTagName('button');
        depositBtn.addEventListener("click", () => window.location.href = "/deposit");
        withdrawBtn.addEventListener("click", () => window.location.href = "/withdraw");
    };
}

function timeStampToTime(unix_timestamp) {
    const date = new Date(unix_timestamp * 1000);
    const dateStr = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
    const timeStr = date.toLocaleTimeString();
    return `${dateStr} ${timeStr}`;
}

async function loadMoreHistory() {
    const history = await getHistory(
        historyPagesToLoad * historyPageSize - 1,
        historyPageSize - 1
    );
    const historyDrawUpdate = historyTemplate(history);
    const historyElem = document.getElementById("history-table");
    historyElem.insertAdjacentHTML('beforeend', historyDrawUpdate);
    enableCopyBtns(historyElem);
    historyPagesToLoad += 1;
}

async function updateLoop() {
    await Promise.allSettled([loadBalance(), loadHistory()]);

    const [jsonresBTC, jsonresETH, jsonresUSDT] = await Promise.allSettled([
        getCourseForCurrency(currencyNameToCurrency("BTC")),
        getCourseForCurrency(currencyNameToCurrency("ETH")),
        getCourseForCurrency(currencyNameToCurrency("USDT"))
    ]);

    const btcTicker = jsonresBTC.value;
    const ethTicker = jsonresETH.value;
    const usdtTicker = jsonresUSDT.value;

    const usdToBtc = document.getElementById("usd-BTC");
    const currValBtc = document.getElementById("curr-val-BTC").textContent;
    usdToBtc.textContent = `(${(currValBtc * btcTicker.USD).toFixed(2)} USD)`

    const usdToEth = document.getElementById("usd-ETH");
    const currValEth = document.getElementById("curr-val-ETH").textContent;
    usdToEth.textContent = `(${(currValEth * ethTicker.USD).toFixed(2)} USD)`;

    const usdToUSDT = document.getElementById("usd-USDT");
    const currValUSDT = document.getElementById("curr-val-USDT").textContent;
    usdToUSDT.textContent = `(${(currValUSDT * 1.0).toFixed(2)} USD)`;


    const awBal = parseFloat(currValUSDT) + currValEth * ethTicker.USD + currValBtc * btcTicker.USD;
    const awBalRub = currValUSDT * usdtTicker.RUB + currValEth * ethTicker.RUB + currValBtc * btcTicker.RUB;

    const totalUsd = document.getElementById("total-balance-usd");
    const totalRub = document.getElementById("total-balance-rub");
    totalUsd.textContent = `${awBal.toFixed(2)} $`;
    totalRub.textContent = `(${awBalRub.toFixed(2)} ₽)`

    await new Promise((resolve) => setTimeout(resolve, refreshInterval));
    updateLoop();
}

async function init() {
    await initTemplates();

    const loadMoreButton = document.getElementById("loadMore");
    loadMoreButton.onclick = loadMoreHistory;
    updateLoop();
};


document.addEventListener("headerLoaded", init);
