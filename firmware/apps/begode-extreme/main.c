/*
 * Begode Extreme main-board firmware: application entry.
 *
 * M0 image: it proves the build, the memory layout and the .euc_info block, nothing more. It configures no
 * peripheral and drives no pin, so it cannot switch the gate drivers or the power latch. The init order and
 * the task table (TIM8 / ADC / RTC / main-loop contexts) arrive here from F1 on.
 */

int main(void)
{
    for (;;) {
    }
}
