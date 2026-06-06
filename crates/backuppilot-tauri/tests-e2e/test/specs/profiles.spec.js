// E2E tests for the Profiles page.
// Prerequisites:
//   - backuppilot-daemon running
//   - cargo install tauri-driver
//   - Windows: msedgedriver.exe on PATH matching installed Edge/WebView2 version
// Run: npm test  (from tests-e2e/)

// Nav buttons render as "<emoji> Label" so use partial text match
const navBtn = (label) => $(`button*=${label}`)

describe('Backup Profiles page', () => {
  it('loads and shows the Profiles heading', async () => {
    const heading = await $('h1.page-title')
    await expect(heading).toBeDisplayed()
    await expect(heading).toHaveText('Backup Profiles')
  })

  it('shows the + Add Profile button', async () => {
    const btn = await $('button*=Add Profile')
    await expect(btn).toBeDisplayed()
  })

  it('shows all four sidebar navigation items', async () => {
    const items = await $$('.nav-item')
    expect(items.length).toBe(4)
    const texts = await Promise.all(items.map(el => el.getText()))
    expect(texts.join(' ')).toMatch(/Profiles/)
    expect(texts.join(' ')).toMatch(/Activity/)
    expect(texts.join(' ')).toMatch(/Restore/)
    expect(texts.join(' ')).toMatch(/Settings/)
  })
})

describe('Add Profile form', () => {
  async function openForm() {
    const btn = await $('button*=Add Profile')
    await btn.click()
    await $('h2=New Backup Profile').waitForDisplayed({ timeout: 5000 })
  }

  async function closeWithCancel() {
    const cancel = await $('button=Cancel')
    await cancel.click()
    await $('h2=New Backup Profile').waitForDisplayed({
      timeout: 3000,
      reverse: true,
    })
  }

  it('opens when the + Add Profile button is clicked', async () => {
    await openForm()
    await expect($('h2=New Backup Profile')).toBeDisplayed()
    await closeWithCancel()
  })

  it('contains all required form sections', async () => {
    await openForm()

    const sections = await $$('.section-title')
    const titles = await Promise.all(sections.map(el => el.getText()))
    expect(titles).toEqual(
      expect.arrayContaining(['GENERAL', 'PBS CONNECTION', 'BACKUP', 'SCHEDULE'])
    )

    await closeWithCancel()
  })

  it('closes when Cancel is clicked', async () => {
    await openForm()
    await closeWithCancel()
    await expect($('h2=New Backup Profile')).not.toBeDisplayed()
  })

  it('closes when Escape is pressed', async () => {
    await openForm()
    await browser.keys('Escape')
    await $('h2=New Backup Profile').waitForDisplayed({
      timeout: 3000,
      reverse: true,
    })
    await expect($('h2=New Backup Profile')).not.toBeDisplayed()
  })

  it('shows a validation error when submitted empty', async () => {
    await openForm()

    const submit = await $('button=Create Profile')
    await submit.click()

    const errorBox = await $('.error-box')
    await expect(errorBox).toBeDisplayed()
    const msg = await errorBox.getText()
    expect(msg.toLowerCase()).toContain('required')

    await closeWithCancel()
  })

  it('passes validation with all required fields filled', async () => {
    await openForm()

    await $('input[placeholder="My Backup"]').setValue('Test Profile')
    await $('input[placeholder="192.168.1.100 or pbs.example.com"]').setValue('192.168.1.100')
    await $('input[placeholder="backups"]').setValue('test-store')
    await $('input[placeholder="tokenid=secret"]').setValue('root@pam!token=mysecret')

    // Add a backup path
    const pathInput = await $('input[placeholder*="Documents"]')
    await pathInput.setValue('C:\\Users\\user\\Documents')
    await $('button=Add').click()

    const submit = await $('button=Create Profile')
    await submit.click()

    await browser.pause(2000)

    // If an error is shown, it must not be a validation error (field-required)
    // It may be an IPC error if daemon is unreachable - that's acceptable here
    const errorBox = await $('.error-box')
    const hasError = await errorBox.isDisplayed().catch(() => false)
    if (hasError) {
      const msg = await errorBox.getText()
      expect(msg.toLowerCase()).not.toContain('required')
    }

    // Clean up
    const cancelBtn = await $('button=Cancel')
    const cancelVisible = await cancelBtn.isDisplayed().catch(() => false)
    if (cancelVisible) await cancelBtn.click()
  })
})

describe('Sidebar navigation', () => {
  it('switches to Activity page', async () => {
    await navBtn('Activity').click()
    const heading = await $('h1.page-title')
    await heading.waitForDisplayed({ timeout: 3000 })
    await expect(heading).toHaveText('Recent Activity')
  })

  it('switches back to Profiles page', async () => {
    await navBtn('Profiles').click()
    const heading = await $('h1.page-title')
    await heading.waitForDisplayed({ timeout: 3000 })
    await expect(heading).toHaveText('Backup Profiles')
  })
})
