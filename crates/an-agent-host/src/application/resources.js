// Operator-owned storage controls. Inventory is advisory; submission rechecks every reference.
let resourceInventory = null;

function resourceButton(label, callback) {
  const button = document.createElement('button');
  button.textContent = label;
  button.onclick = () => act(callback);
  return button;
}

function resourceSize(value) {
  return value === null ? strings[language].unknown : `${(value / 1048576).toFixed(1)} MiB`;
}

async function showResources() {
  const environment = $('image-environment').value;
  if (!environment) throw new Error(strings[language].noEnvironment);
  resourceInventory = await request('/action', {op: 'image-inventory', environment});
  renderResources();
}

function renderResources() {
  const list = $('resource-list');
  const environment = resourceInventory.environment;
  list.replaceChildren();
  if (!resourceInventory.supported) {
    list.textContent = strings[language].maintenanceUnsupported;
    return;
  }
  const notice = document.createElement('p');
  notice.textContent = strings[language].storageNotice;
  list.append(notice);
  for (const resource of resourceInventory.resources) renderResource(list, resource, environment);
}

function renderResource(list, resource, environment) {
  const row = document.createElement('div');
  const label = document.createElement('p');
  const kind = strings[language][resource.kind === 'image' ? 'diskImage' : 'taskCopy'];
  label.textContent = `${kind} ${resource.id.slice(0, 8)} · ${strings[language].logicalSize}: ` +
    `${resourceSize(resource.logical_bytes)} · ${strings[language].allocatedSize}: ${resourceSize(resource.allocated_bytes)}`;
  row.append(label);
  if (resource.reclaimable) {
    row.append(resourceButton(strings[language].previewCleanup, () => prepareImage(
      resource.kind === 'image' ? 'retire' : 'reclaim_vm', {environment, resource: resource.id})));
  } else {
    const reason = document.createElement('small');
    reason.textContent = resource.blocked_by.map(key => strings[language]['resource_' + key] || strings[language].resourceBlocked).join(' · ');
    row.append(reason);
  }
  list.append(row);
}

$('resources').onclick = () => act(showResources);

$('language').addEventListener('change', () => { if (resourceInventory) renderResources(); });
$('image-environment').addEventListener('change', () => {
  resourceInventory = null;
  $('resource-list').replaceChildren();
});
