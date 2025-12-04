<script lang="ts">
  import { Toggle } from '../components';

  // Local sound setting (will be wired to real settings store later)
  let soundEnabled = $state(true);
</script>

<section class="tab-content">
  <div class="settings-section">
    <h2 class="section-title">Sound</h2>
    
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Enable sounds</span>
        <span class="setting-hint">Play sound effects for item drops</span>
      </div>
      <Toggle bind:checked={soundEnabled} />
    </div>
  </div>
</section>


