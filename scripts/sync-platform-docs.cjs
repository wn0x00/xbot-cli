#!/usr/bin/env node
'use strict';

// Refresh only the public documents explicitly included in the platform integration.
const fs = require('node:fs');
const path = require('node:path');

const ids = `
710460304655855616 958294294025375744 710465687966658560 971279704374095872
710499792859115520 710494771633700864 710498885910380544 710497757007953920
710496549620625408 710495670001700864 710494844581036032 710472225308225536
710474957390741504 971288732102688768 971292986109808640 971293347837771776
971293556891668480 710472302045138944 710473073333092352 971294724116140032
710475693074886656 710477790566137856 710476864272334848 710475766593617920
710483080343138304 710485133474119680 710487379305533440 710488569666060288
710486342769258496 710484124829691904 710483153807343616 710489540981555200
710465801475309568 710466731559485440 710492920473436160 710490754371760128
710491717794512896 710489613909069824 710468955701456896 710469888978161664
710468004680773632 710479860700229632 710481967730900992 710481123051515904
710479934544805888 712541081694265344 971304783746039808 971305176334413824
710494036079095808 717268028778450944 717268756412026880 717267384361996288
971305566201839616 971305676046467072 971305358878912512 712489730706411520
710470879923273728 971303882939564032 971307981009453056 710474186124374016
710460881831145472 912893828744491008 710463081711124480 710462821173268480
710462530950475776 710462272255164416 710460954567454720 710460379288813568
`.trim().split(/\s+/);

function text(node) {
  if (node.type === 'hardBreak') return '\n';
  return (node.text || '') + (node.content || []).map(text).join('');
}

function blocks(node) {
  if (!node || typeof node !== 'object') return [];
  if (node.type === 'table') {
    return [{ type: 'table', rows: node.content.map(row => row.content.map(text)) }];
  }
  if (['heading', 'paragraph', 'codeBlock'].includes(node.type)) {
    const value = text(node).trim();
    return value ? [{ type: node.type, text: value }] : [];
  }
  return (node.content || []).flatMap(blocks);
}

function apiTables(content) {
  let heading = '';
  const result = [];
  for (const block of blocks(content)) {
    if (block.type === 'heading') heading = block.text;
    if (block.type !== 'table') continue;
    // Keep factual API contracts; prose, FAQ answers, examples and attachments
    // remain at their original documentation URLs.
    const first = block.rows[0] || [];
    if (!first.some(cell => /^(基本|名称|参数|错误码|状态码|接口)/.test(cell))
        && !block.rows.some(row => row[0]?.startsWith('/oapi/'))) continue;
    result.push({ type: 'heading', text: heading }, block);
  }
  return result;
}

async function readDoc(id) {
  const url = `https://www.yingdao.com/yddoc/rpa/zh-CN/${id}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(45000) });
  if (!response.ok) throw new Error(`${id}: HTTP ${response.status}`);
  const html = await response.text();
  const match = html.match(/<script id="__NEXT_DATA__" type="application\/json">([\s\S]*?)<\/script>/);
  if (!match) throw new Error(`${id}: missing document data`);
  const props = JSON.parse(match[1]).props.pageProps;
  if (!props.docContent?.name) throw new Error(`${id}: missing document title`);
  let content = props.contentJson;
  if (typeof content === 'string' && content) content = JSON.parse(content);
  const result = { id, title: props.docContent.name, url, blocks: apiTables(content) };
  if (props.docContent.content && !blocks(content).length) {
    throw new Error(`${id}: document body was not returned`);
  }
  return result;
}

async function main() {
  const documents = [];
  // Small batches avoid placing unnecessary load on the public documentation service.
  for (let offset = 0; offset < ids.length; offset += 6) {
    const batch = await Promise.all(ids.slice(offset, offset + 6).map(readDoc));
    documents.push(...batch);
    for (const doc of batch) console.log(`${doc.id} ${doc.title} (${doc.blocks.length} blocks)`);
  }
  const destination = path.join(__dirname, '..', 'docs', 'platform-api-reference.json');
  fs.writeFileSync(destination, JSON.stringify({
    source: 'Yingdao public RPA Open API documentation',
    documents,
  }, null, 2) + '\n');
}

main().catch(error => {
  console.error(error.message);
  process.exitCode = 1;
});
